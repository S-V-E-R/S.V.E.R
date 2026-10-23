//! Go-live alerts (docs/LIVE_STREAMS.md "Go-live alerts"): in-site notifications, viewer browser
//! push, phone push through Expo, and opt-in email. The jobs pass fans out each broadcast once,
//! when it first reaches LIVE. Phone push uses the same switches as browser push.
use crate::{
    App,
    profiles::{self, Fail, Res, signed_in},
    security as sec,
    staff_push::{self, Sent, Subscription},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

pub const COMMUNITY_TYPES: [&str; 6] = [
    "guild_application",
    "guild_decision",
    "guild_invite",
    "squad_invite",
    "dm",
    "light",
];
/// A durable community event. Its stable key also deduplicates push work on retries.
#[allow(clippy::too_many_arguments)]
pub async fn community(
    db: &mut sqlx::PgConnection,
    recipient: &str,
    actor: &str,
    kind: &str,
    key: &str,
    guild: Option<&str>,
    payload: &Value,
) -> Res<()> {
    if !COMMUNITY_TYPES.contains(&kind) {
        return Err(Fail::internal());
    }
    if recipient == actor || profiles::blocked_between(db, recipient, actor).await? {
        return Ok(());
    }
    if profiles::channel_user_by_id(db, recipient)
        .await?
        .is_none_or(|u| !u.eligible)
    {
        return Ok(());
    }
    let (site,push):(bool,bool)=sqlx::query_as("SELECT coalesce(t.site,true),coalesce(t.push,true) FROM (SELECT 1) one LEFT JOIN notification_type_settings t ON t.user_id=$1 AND t.kind=$2")
        .bind(recipient).bind(kind).fetch_one(&mut *db).await?;
    if !site && !push {
        return Ok(());
    }
    let id = profiles::new_id();
    let inserted=sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,event_key,payload,guild_id,site_visible) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING")
        .bind(&id).bind(recipient).bind(kind).bind(actor).bind(format!("{kind}:{key}")).bind(payload).bind(guild).bind(site).execute(&mut *db).await?.rows_affected();
    if inserted > 0 && push {
        sqlx::query("INSERT INTO push_jobs(id,subscription_id,notification_id) SELECT gen_random_uuid()::text,id,$2 FROM push_subscriptions WHERE user_id=$1 ON CONFLICT DO NOTHING")
            .bind(recipient).bind(&id).execute(&mut *db).await?;
        sqlx::query("INSERT INTO expo_push_jobs(id,device_id,notification_id) SELECT gen_random_uuid()::text,id,$2 FROM expo_devices WHERE user_id=$1 ON CONFLICT DO NOTHING")
            .bind(recipient).bind(&id).execute(&mut *db).await?;
    }
    Ok(())
}

async fn deliver_community(app: &App) -> Res<()> {
    type PushRow = (
        String,
        String,
        String,
        i32,
        String,
        String,
        String,
        Option<String>,
        Value,
    );
    let Some(key) = staff_push::signing_key(app)? else {
        return Ok(());
    };
    for _ in 0..50 {
        let mut tx = app.db.begin().await?;
        let row:Option<PushRow>=sqlx::query_as("SELECT j.id,s.id,s.subscription,j.attempts,n.user_id,n.channel_id,n.kind,n.guild_id,n.payload FROM push_jobs j JOIN notifications n ON n.id=j.notification_id JOIN push_subscriptions s ON s.id=j.subscription_id WHERE j.available_at<=now() ORDER BY j.available_at FOR UPDATE OF j SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some((job, subscription, sealed, attempts, user, actor, kind, guild, mut payload)) =
            row
        else {
            break;
        };
        let enabled:bool=sqlx::query_scalar("SELECT coalesce((SELECT push FROM notification_type_settings WHERE user_id=$1 AND kind=$2),true)").bind(&user).bind(&kind).fetch_one(&mut *tx).await?;
        let allowed = enabled
            && !profiles::blocked_between(&mut tx, &user, &actor).await?
            && profiles::channel_user_by_id(&mut tx, &user)
                .await?
                .is_some_and(|u| u.eligible)
            && profiles::channel_user_by_id(&mut tx, &actor)
                .await?
                .is_some_and(|u| u.eligible)
            && match &guild {
                Some(g) => crate::guilds::notify_allowed(&mut tx, g, &user).await?,
                None => true,
            };
        let input: Option<Subscription> = sec::unseal(app, "push", &sealed)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        payload["id"] = json!(job);
        payload["tag"] = json!(format!("sver-{kind}-{job}"));
        let sent = if !allowed {
            Sent::Accepted
        } else if let Some(input) = input {
            staff_push::send(app, &key, &input, &job, &payload)
                .await
                .unwrap_or(Sent::Retry)
        } else {
            Sent::Gone
        };
        match sent {
            Sent::Accepted => {
                sqlx::query("DELETE FROM push_jobs WHERE id=$1")
                    .bind(&job)
                    .execute(&mut *tx)
                    .await?;
            }
            Sent::Gone => {
                sqlx::query("DELETE FROM push_subscriptions WHERE id=$1")
                    .bind(&subscription)
                    .execute(&mut *tx)
                    .await?;
            }
            Sent::Retry => {
                sqlx::query("UPDATE push_jobs SET attempts=attempts+1,available_at=now()+make_interval(secs=>$2) WHERE id=$1").bind(&job).bind(f64::from(30*2_i32.pow(attempts.min(4) as u32))).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

/// Fans out broadcasts that reached LIVE and have not alerted yet. Each broadcast is handled in one
/// transaction that also marks it, so a retry never sends twice.
pub async fn fan_out(app: &App) -> Res<()> {
    sqlx::query(
        "UPDATE broadcasts SET alert_state='skipped' WHERE alert_state IS NULL AND state='ENDED'",
    )
    .execute(&app.db)
    .await?;
    loop {
        let mut tx = app.db.begin().await?;
        let row: Option<(String, String)> = sqlx::query_as("SELECT id,owner_id FROM broadcasts WHERE alert_state IS NULL AND state='LIVE' ORDER BY started_at FOR UPDATE SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
        let Some((broadcast, owner)) = row else {
            return Ok(());
        };
        // Live events: handled once per broadcast here, whether or not alerts are throttled.
        crate::events::emit(&mut tx, &owner, "live", json!({"live": true})).await?;
        // At most one alert per channel every 6 hours; restricted channels send nothing.
        let channel: Option<(String, String)> = sqlx::query_as("SELECT c.username,c.display_name FROM channel_users c, broadcasts b WHERE c.id=$2 AND c.eligible AND b.id=$1 AND NOT EXISTS(SELECT 1 FROM broadcasts p WHERE p.owner_id=b.owner_id AND p.alert_state='sent' AND p.started_at>b.started_at-interval '6 hours')")
            .bind(&broadcast)
            .bind(&owner)
            .fetch_optional(&mut *tx)
            .await?;
        let Some((username, display_name)) = channel else {
            sqlx::query("UPDATE broadcasts SET alert_state='throttled' WHERE id=$1")
                .bind(&broadcast)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            continue;
        };
        // Followers with alerts on for this channel; never the owner, people the owner blocked or
        // banned from the channel, unverified, restricted or deleted accounts.
        let recipients: Vec<(String, bool, bool, bool, String)> = sqlx::query_as("SELECT c.id,coalesce(s.site,true),coalesce(s.push,true),coalesce(s.email,false),u.email FROM follows f JOIN channel_users c ON c.id=f.follower_id JOIN users u ON u.id=f.follower_id LEFT JOIN notification_settings s ON s.user_id=f.follower_id WHERE f.following_id=$1 AND f.alerts AND c.eligible AND c.email_verified AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE k.blocker_id=$1 AND k.blocked_id=f.follower_id) AND NOT EXISTS(SELECT 1 FROM channel_restrictions r WHERE r.channel_id=$1 AND r.user_id=f.follower_id AND r.kind='ban') AND NOT (coalesce((SELECT mature FROM stream_settings WHERE owner_id=$1),false) AND coalesce(u.date_of_birth>current_date-interval '18 years',true))")
            .bind(&owner)
            .fetch_all(&mut *tx)
            .await?;
        let pick = |n: usize| -> Vec<String> {
            recipients
                .iter()
                .filter(|r| [r.1, r.2, r.3][n])
                .map(|r| r.0.clone())
                .collect()
        };
        sqlx::query("INSERT INTO notifications(id,user_id,kind,channel_id,broadcast_id) SELECT gen_random_uuid()::text,u,'live',$2,$3 FROM unnest($1::text[]) u ON CONFLICT DO NOTHING")
            .bind(pick(0)).bind(&owner).bind(&broadcast).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO push_jobs(id,subscription_id,broadcast_id) SELECT gen_random_uuid()::text,s.id,$2 FROM push_subscriptions s WHERE s.user_id=ANY($1) ON CONFLICT DO NOTHING")
            .bind(pick(1)).bind(&broadcast).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO expo_push_jobs(id,device_id,broadcast_id) SELECT gen_random_uuid()::text,d.id,$2 FROM expo_devices d WHERE d.user_id=ANY($1) ON CONFLICT DO NOTHING")
            .bind(pick(1)).bind(&broadcast).execute(&mut *tx).await?;
        for (id, _, _, _, email) in recipients.iter().filter(|r| r.3) {
            // The header is the RFC 8058 one-click POST; the visible link opens a confirm page.
            let token = encode(&sec::seal(app, "unsubscribe", id)?);
            let link = format!(
                "{}/api/notifications/unsubscribe?token={token}",
                app.config.origin
            );
            let page = format!("{}/unsubscribe?token={token}", app.config.origin);
            let subject = format!("{display_name} is live on S.V.E.R");
            let text = format!(
                "{subject}\n\nWatch: {}/{username}\n\nYou follow this channel and turned on go-live emails. Stop these emails: {page}\nYou can also change alerts in your notification settings: {}/settings/notifications\n\n{}",
                app.config.origin, app.config.origin, app.config.mail_postal_address
            );
            let payload = json!({"to":[email],"subject":subject,"text":text,
                "headers":{"List-Unsubscribe":format!("<{link}>"),"List-Unsubscribe-Post":"List-Unsubscribe=One-Click"}});
            // A go-live email older than two hours is no longer useful.
            sqlx::query("INSERT INTO mail_jobs(id,user_id,payload,expires_at) VALUES($1,$2,$3,now()+interval '2 hours')")
                .bind(profiles::new_id()).bind(id).bind(sec::seal(app, "mail", &payload.to_string())?).execute(&mut *tx).await?;
        }
        // The channel's own Discord server, under the same throttle (docs/COMMUNITY.md "Discord bot").
        sqlx::query("INSERT INTO discord_posts(broadcast_id,channel_id) SELECT $1,channel_id FROM discord_servers WHERE channel_id=$2 AND post_channel IS NOT NULL ON CONFLICT DO NOTHING")
            .bind(&broadcast).bind(&owner).execute(&mut *tx).await?;
        sqlx::query("UPDATE broadcasts SET alert_state='sent' WHERE id=$1")
            .bind(&broadcast)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
}
fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Delivers queued go-live push messages and applies retention.
pub async fn deliver(app: &App) -> Res<()> {
    sqlx::query("DELETE FROM notifications WHERE created_at<now()-interval '30 days'")
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM push_jobs WHERE notification_id IS NOT NULL AND created_at<now()-interval '1 day'").execute(&app.db).await?;
    deliver_community(app).await?;
    // A go-live push for a stream that ended, or older than an hour, is dropped.
    sqlx::query("DELETE FROM push_jobs j USING broadcasts b WHERE b.id=j.broadcast_id AND (b.state='ENDED' OR j.created_at<now()-interval '1 hour')")
        .execute(&app.db)
        .await?;
    // Phone push does not use the browser VAPID key. An empty Expo token leaves jobs queued.
    deliver_expo(app).await?;
    let Some(key) = staff_push::signing_key(app)? else {
        return Ok(());
    };
    for _ in 0..50 {
        let mut tx = app.db.begin().await?;
        let row: Option<(String, String, String, i32, String, String)> = sqlx::query_as("SELECT j.id,s.id,s.subscription,j.attempts,c.username,c.display_name FROM push_jobs j JOIN push_subscriptions s ON s.id=j.subscription_id JOIN users u ON u.id=s.user_id AND u.deleted_at IS NULL JOIN broadcasts b ON b.id=j.broadcast_id JOIN channel_users c ON c.id=b.owner_id WHERE j.available_at<=now() ORDER BY j.available_at FOR UPDATE OF j SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
        let Some((job, subscription, sealed, attempts, username, display_name)) = row else {
            break;
        };
        let input: Option<Subscription> = sec::unseal(app, "push", &sealed)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        let payload = json!({"id":job,"tag":format!("sver-live-{}",username.to_lowercase()),
            "title":format!("{display_name} is live"),"body":"Watch now on S.V.E.R.","url":format!("/{username}")});
        let sent = match &input {
            // A failed send (timeout, push service down) retries; only the service saying the
            // subscription is gone removes it.
            Some(input) => staff_push::send(app, &key, input, &job, &payload)
                .await
                .unwrap_or(Sent::Retry),
            None => Sent::Gone,
        };
        match sent {
            Sent::Accepted => {
                sqlx::query("DELETE FROM push_jobs WHERE id=$1")
                    .bind(&job)
                    .execute(&mut *tx)
                    .await?;
            }
            // The browser unsubscribed (or the stored subscription is unusable): forget it.
            Sent::Gone => {
                sqlx::query("DELETE FROM push_subscriptions WHERE id=$1")
                    .bind(&subscription)
                    .execute(&mut *tx)
                    .await?;
            }
            Sent::Retry => {
                sqlx::query("UPDATE push_jobs SET attempts=attempts+1,available_at=now()+make_interval(secs=>$2) WHERE id=$1")
                    .bind(&job).bind(f64::from(30 * 2_i32.pow(attempts.min(4) as u32))).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

/// id, kind, created_at, read, still live, channel username, display name, avatar key.
type Row = (
    String,
    String,
    DateTime<Utc>,
    bool,
    bool,
    String,
    String,
    Option<String>,
);
/// GET /api/me/notifications: the last 30 days, newest first.
async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let rows: Vec<Row> = sqlx::query_as("SELECT n.id,n.kind,n.created_at,n.read_at IS NOT NULL,b.state<>'ENDED',c.username,c.display_name,c.avatar_key FROM notifications n JOIN channel_users c ON c.id=n.channel_id AND c.eligible JOIN broadcasts b ON b.id=n.broadcast_id WHERE n.user_id=$1 AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$1 AND k.blocked_id=n.channel_id) OR (k.blocker_id=n.channel_id AND k.blocked_id=$1)) ORDER BY n.created_at DESC LIMIT 50")
        .bind(&user.id)
        .fetch_all(&app.db)
        .await?;
    let mut items: Vec<Value> = rows.into_iter().map(|(id,kind,at,read,live,username,display_name,avatar)| json!({"id":id,"kind":kind,"created_at":at,"read":read,"live":live,
        "channel":{"username":username,"display_name":display_name,"avatar":profiles::avatar_json(&app,avatar.as_deref())}})).collect();
    let mut db = app.db.acquire().await?;
    let other:Vec<(Option<String>,Value)>=sqlx::query_as("SELECT n.guild_id,jsonb_build_object('id',n.id,'kind',n.kind,'created_at',n.created_at,'read',n.read_at IS NOT NULL,'payload',n.payload) FROM notifications n JOIN channel_users c ON c.id=n.channel_id AND c.eligible WHERE n.user_id=$1 AND n.site_visible AND n.kind<>'live' AND NOT EXISTS(SELECT 1 FROM user_blocks k WHERE (k.blocker_id=$1 AND k.blocked_id=n.channel_id) OR (k.blocker_id=n.channel_id AND k.blocked_id=$1)) ORDER BY n.created_at DESC LIMIT 50")
        .bind(&user.id).fetch_all(&mut *db).await?;
    for (guild, item) in other {
        if let Some(g) = guild
            && !crate::guilds::notify_allowed(&mut db, &g, &user.id).await?
        {
            continue;
        }
        items.push(item);
    }
    items.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    items.truncate(50);
    let unread = items.iter().filter(|i| i["read"] == false).count();
    Ok(Json(json!({"items":items,"unread":unread})))
}
/// Unread in-site notifications, for the top-bar bell.
pub async fn unread(app: &App, user: &str) -> Res<i64> {
    let mut db = app.db.acquire().await?;
    let rows:Vec<Option<String>>=sqlx::query_scalar("SELECT n.guild_id FROM notifications n JOIN channel_users c ON c.id=n.channel_id AND c.eligible WHERE n.user_id=$1 AND n.read_at IS NULL AND n.site_visible AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=n.channel_id) OR (b.blocker_id=n.channel_id AND b.blocked_id=$1))")
        .bind(user).fetch_all(&mut *db).await?;
    let mut count = 0;
    for guild in rows {
        if let Some(g) = guild
            && !crate::guilds::notify_allowed(&mut db, &g, user).await?
        {
            continue;
        }
        count += 1;
    }
    Ok(count)
}

/// POST /api/me/notifications/read
async fn mark_read(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    sqlx::query("UPDATE notifications SET read_at=now() WHERE user_id=$1 AND read_at IS NULL")
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"unread":0})))
}

#[derive(Deserialize)]
struct Settings {
    site: bool,
    push: bool,
    email: bool,
    #[serde(default)]
    community: Vec<TypeSettings>,
}
#[derive(Deserialize)]
struct TypeSettings {
    kind: String,
    site: bool,
    push: bool,
}
async fn settings_json(app: &App, user: &str) -> Res<Json<Value>> {
    let (site, push, email, devices, phones): (bool, bool, bool, i64, i64) = sqlx::query_as("SELECT coalesce(s.site,true),coalesce(s.push,true),coalesce(s.email,false),(SELECT count(*) FROM push_subscriptions WHERE user_id=$1),(SELECT count(*) FROM expo_devices WHERE user_id=$1) FROM (SELECT 1) one LEFT JOIN notification_settings s ON s.user_id=$1")
        .bind(user)
        .fetch_one(&app.db)
        .await?;
    let community:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('kind',k,'site',coalesce(t.site,true),'push',coalesce(t.push,true)) FROM unnest($2::text[]) k LEFT JOIN notification_type_settings t ON t.user_id=$1 AND t.kind=k")
        .bind(user).bind(COMMUNITY_TYPES.to_vec()).fetch_all(&app.db).await?;
    Ok(Json(
        json!({"site":site,"push":push,"email":email,"push_devices":devices,"expo_devices":phones,"push_key":app.config.staff_push.public_key,"community":community}),
    ))
}
/// GET /api/me/notifications/settings
async fn get_settings(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    settings_json(&app, &user.id).await
}
/// PUT /api/me/notifications/settings
async fn put_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if input.email && !user.email_verified {
        return Err(Fail::bad(
            "Verify your email address before turning on email alerts.",
        ));
    }
    let mut kinds = std::collections::HashSet::new();
    if input.community.len() > COMMUNITY_TYPES.len()
        || input
            .community
            .iter()
            .any(|t| !COMMUNITY_TYPES.contains(&t.kind.as_str()) || !kinds.insert(&t.kind))
    {
        return Err(Fail::bad("Invalid or repeated notification type."));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO notification_settings(user_id,site,push,email) VALUES($1,$2,$3,$4) ON CONFLICT(user_id) DO UPDATE SET site=$2,push=$3,email=$4")
        .bind(&user.id).bind(input.site).bind(input.push).bind(input.email).execute(&mut *tx).await?;
    for t in input.community {
        sqlx::query("INSERT INTO notification_type_settings(user_id,kind,site,push) VALUES($1,$2,$3,$4) ON CONFLICT(user_id,kind) DO UPDATE SET site=$3,push=$4")
            .bind(&user.id).bind(t.kind).bind(t.site).bind(t.push).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    settings_json(&app, &user.id).await
}

/// POST /api/me/push: saves this browser's push subscription (at most ten per account).
async fn subscribe(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Subscription>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    staff_push::validate(&input)?;
    if app.config.staff_push.private_key.is_empty() {
        return Err(Fail::unavailable("Push alerts are not available yet."));
    }
    let id = sec::digest(&input.endpoint);
    let mut tx = app.db.begin().await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM push_subscriptions WHERE user_id=$1 AND id<>$2")
            .bind(&user.id)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    if count >= 10 {
        return Err(Fail::bad(
            "Push alerts are on for ten browsers already. Turn one off first.",
        ));
    }
    let sealed = sec::seal(
        &app,
        "push",
        &serde_json::to_string(&input).map_err(|_| Fail::internal())?,
    )?;
    sqlx::query("INSERT INTO push_subscriptions(id,user_id,subscription) VALUES($1,$2,$3) ON CONFLICT(id) DO UPDATE SET user_id=EXCLUDED.user_id,subscription=EXCLUDED.subscription")
        .bind(&id).bind(&user.id).bind(sealed).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
#[derive(Deserialize)]
struct Endpoint {
    endpoint: String,
}
/// DELETE /api/me/push: forgets this browser's subscription.
async fn unsubscribe_push(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Endpoint>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM push_subscriptions WHERE id=$1 AND user_id=$2")
        .bind(sec::digest(&input.endpoint))
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"saved":false})))
}

#[derive(Deserialize)]
struct FollowAlerts {
    alerts: bool,
}
/// PATCH /api/channels/{username}/follow: turns alerts for one followed channel on or off.
async fn follow_alerts(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<FollowAlerts>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let channel = profiles::eligible_by_name(&mut db, &name)
        .await?
        .ok_or_else(Fail::channel_missing)?;
    let updated =
        sqlx::query("UPDATE follows SET alerts=$3 WHERE follower_id=$1 AND following_id=$2")
            .bind(&user.id)
            .bind(&channel.id)
            .bind(input.alerts)
            .execute(&mut *db)
            .await?
            .rows_affected();
    if updated == 0 {
        return Err(Fail::bad("Follow this channel to get its alerts."));
    }
    Ok(Json(json!({"following":true,"alerts":input.alerts})))
}

#[derive(Deserialize)]
struct Token {
    token: String,
}
/// POST /api/notifications/unsubscribe?token=: the one-click email unsubscribe (RFC 8058). The
/// token names the account; it only ever turns go-live email off.
async fn unsubscribe_email(State(app): State<App>, Query(input): Query<Token>) -> Res<Json<Value>> {
    let user = sec::unseal(&app, "unsubscribe", &input.token)
        .map_err(|_| Fail::bad("This unsubscribe link is not valid."))?;
    sqlx::query("INSERT INTO notification_settings(user_id,email) SELECT id,false FROM users WHERE id=$1 ON CONFLICT(user_id) DO UPDATE SET email=false")
        .bind(user)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"unsubscribed":true})))
}

/// `ExponentPushToken[...]` or `ExpoPushToken[...]`. Anything else is refused, so the API never
/// stores an arbitrary URL or a native FCM/APNs token it would not know how to address.
fn valid_expo_token(token: &str) -> bool {
    let inner = token
        .strip_prefix("ExponentPushToken[")
        .or_else(|| token.strip_prefix("ExpoPushToken["))
        .and_then(|rest| rest.strip_suffix(']'));
    inner.is_some_and(|inner| {
        (16..=200).contains(&inner.len())
            && inner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    })
}

#[derive(Deserialize)]
struct ExpoToken {
    token: String,
}

/// POST /api/me/push/expo: saves this phone's Expo push token (at most ten per account).
async fn register_expo(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ExpoToken>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if !valid_expo_token(&input.token) {
        return Err(Fail::bad("That push token is not valid."));
    }
    let id = sec::digest(&input.token);
    let sealed = sec::seal(&app, "expo-push", &input.token)?;
    let mut tx = app.db.begin().await?;
    // A phone that signs into a different account must not deliver the previous queue.
    sqlx::query("DELETE FROM expo_push_jobs WHERE device_id=$1 AND EXISTS(SELECT 1 FROM expo_devices WHERE id=$1 AND user_id<>$2)")
        .bind(&id).bind(&user.id).execute(&mut *tx).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM expo_devices WHERE user_id=$1 AND id<>$2")
            .bind(&user.id)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    if count >= 10 {
        return Err(Fail::bad(
            "Push alerts are on for ten phones already. Turn one off first.",
        ));
    }
    sqlx::query("INSERT INTO expo_devices(id,user_id,token) VALUES($1,$2,$3) ON CONFLICT(id) DO UPDATE SET user_id=EXCLUDED.user_id,token=EXCLUDED.token")
        .bind(&id).bind(&user.id).bind(sealed).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"saved": true})))
}

/// DELETE /api/me/push/expo: forgets this phone. Another account's row is left alone.
async fn forget_expo(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<ExpoToken>,
) -> Res<Json<Value>> {
    let user = signed_in(&app, &jar).await?;
    if !valid_expo_token(&input.token) {
        return Err(Fail::bad("That push token is not valid."));
    }
    sqlx::query("DELETE FROM expo_devices WHERE id=$1 AND user_id=$2")
        .bind(sec::digest(&input.token))
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"saved": false})))
}

fn expo_message(token: &str, title: &str, body: &str, url: &str, channel: &str) -> Value {
    json!({
        "to": token,
        "title": title,
        "body": body,
        "data": {"url": url},
        "sound": "default",
        "priority": if channel == "live" { "high" } else { "default" },
        "channelId": channel,
    })
}

fn classify_expo(status: reqwest::StatusCode, body: &Value) -> Sent {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Sent::Retry;
    }
    let ticket = if body["data"].is_array() {
        &body["data"][0]
    } else {
        &body["data"]
    };
    if ticket["details"]["error"].as_str() == Some("DeviceNotRegistered") {
        return Sent::Gone;
    }
    if status.is_success() && ticket["status"] == "ok" {
        return Sent::Accepted;
    }
    Sent::Retry
}

async fn post_expo(app: &App, message: &Value) -> Sent {
    let response = app
        .http
        .post(&app.config.expo_push_url)
        .bearer_auth(&app.config.expo_access_token)
        .json(message)
        .send()
        .await;
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.json::<Value>().await.unwrap_or(Value::Null);
            classify_expo(status, &body)
        }
        Err(_) => Sent::Retry,
    }
}

/// job, device, sealed token, attempts, channel username, display name, kind, payload, recipient, actor, guild.
type ExpoRow = (
    String,
    String,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Value>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Sends queued phone notifications. Kinds that are in-site only never get a job here.
/// Go-live copy is fixed; other kinds reuse the title and body already stored for browser push.
pub async fn deliver_expo(app: &App) -> Res<()> {
    sqlx::query("DELETE FROM expo_push_jobs WHERE notification_id IS NOT NULL AND created_at<now()-interval '1 day'")
        .execute(&app.db).await?;
    sqlx::query("DELETE FROM expo_push_jobs j USING broadcasts b WHERE b.id=j.broadcast_id AND (b.state='ENDED' OR j.created_at<now()-interval '1 hour')")
        .execute(&app.db).await?;
    if app.config.expo_access_token.is_empty() {
        return Ok(());
    }
    for _ in 0..50 {
        let mut tx = app.db.begin().await?;
        let row: Option<ExpoRow> = sqlx::query_as("SELECT j.id,d.id,d.token,j.attempts,c.username,c.display_name,n.kind,n.payload,n.user_id,n.channel_id,n.guild_id FROM expo_push_jobs j JOIN expo_devices d ON d.id=j.device_id JOIN users u ON u.id=d.user_id AND u.deleted_at IS NULL LEFT JOIN broadcasts b ON b.id=j.broadcast_id LEFT JOIN channel_users c ON c.id=b.owner_id LEFT JOIN notifications n ON n.id=j.notification_id WHERE j.available_at<=now() ORDER BY j.available_at FOR UPDATE OF j SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some((
            job,
            device,
            sealed,
            attempts,
            username,
            display_name,
            kind,
            payload,
            recipient,
            actor,
            guild,
        )) = row
        else {
            break;
        };
        let token = match sec::unseal(app, "expo-push", &sealed) {
            Ok(token) if valid_expo_token(&token) => token,
            _ => {
                sqlx::query("DELETE FROM expo_devices WHERE id=$1")
                    .bind(&device)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                continue;
            }
        };
        let allowed = match &kind {
            None => username.is_some(),
            Some(kind) => {
                let user = recipient.as_deref().unwrap_or("");
                let actor = actor.as_deref().unwrap_or("");
                let enabled: bool = sqlx::query_scalar("SELECT coalesce((SELECT push FROM notification_type_settings WHERE user_id=$1 AND kind=$2),true)").bind(user).bind(kind).fetch_one(&mut *tx).await?;
                enabled
                    && !user.is_empty()
                    && !actor.is_empty()
                    && !profiles::blocked_between(&mut tx, user, actor).await?
                    && profiles::channel_user_by_id(&mut tx, user)
                        .await?
                        .is_some_and(|row| row.eligible)
                    && profiles::channel_user_by_id(&mut tx, actor)
                        .await?
                        .is_some_and(|row| row.eligible)
                    && match &guild {
                        Some(id) => crate::guilds::notify_allowed(&mut tx, id, user).await?,
                        None => true,
                    }
            }
        };
        let message = if !allowed {
            None
        } else if let (Some(username), Some(display_name)) = (&username, &display_name) {
            Some(expo_message(
                &token,
                &format!("{display_name} is live"),
                "Watch now on S.V.E.R.",
                &format!("/{username}"),
                "live",
            ))
        } else if let Some(payload) = &payload {
            let title: String = payload["title"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect();
            let body: String = payload["body"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect();
            let url = payload["url"].as_str().unwrap_or("");
            if title.is_empty()
                || url.len() > 500
                || !url.starts_with('/')
                || url.starts_with("//")
                || url.contains("://")
            {
                None
            } else {
                Some(expo_message(&token, &title, &body, url, "alerts"))
            }
        } else {
            None
        };
        let sent = match &message {
            Some(message) => post_expo(app, message).await,
            None => Sent::Accepted,
        };
        match sent {
            Sent::Accepted => {
                sqlx::query("DELETE FROM expo_push_jobs WHERE id=$1")
                    .bind(&job)
                    .execute(&mut *tx)
                    .await?;
            }
            Sent::Gone => {
                sqlx::query("DELETE FROM expo_devices WHERE id=$1")
                    .bind(&device)
                    .execute(&mut *tx)
                    .await?;
            }
            Sent::Retry => {
                sqlx::query("UPDATE expo_push_jobs SET attempts=attempts+1,available_at=now()+make_interval(secs=>$2) WHERE id=$1")
                    .bind(&job).bind(f64::from(30 * 2_i32.pow(attempts.min(4) as u32))).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/notifications", get(list))
        .route("/api/me/notifications/read", post(mark_read))
        .route(
            "/api/me/notifications/settings",
            get(get_settings).put(put_settings),
        )
        .route("/api/me/push", post(subscribe).delete(unsubscribe_push))
        .route("/api/me/push/expo", post(register_expo).delete(forget_expo))
        .route("/api/channels/{username}/follow", patch(follow_alerts))
        .route("/api/notifications/unsubscribe", post(unsubscribe_email))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expo_tokens_and_tickets() {
        assert!(valid_expo_token("ExponentPushToken[alertfana000000000001]"));
        assert!(valid_expo_token("ExpoPushToken[alertfana000000000001]"));
        assert!(!valid_expo_token("https://exp.host/--/api/v2/push/send"));
        assert!(!valid_expo_token("ExponentPushToken[short]"));
        assert!(!valid_expo_token("ExponentPushToken[has space inside]"));
        assert!(matches!(
            classify_expo(
                reqwest::StatusCode::OK,
                &json!({"data":{"status":"ok","id":"synthetic"}})
            ),
            Sent::Accepted
        ));
        assert!(matches!(
            classify_expo(
                reqwest::StatusCode::OK,
                &json!({"data":{"status":"error","details":{"error":"DeviceNotRegistered"}}})
            ),
            Sent::Gone
        ));
        assert!(matches!(
            classify_expo(
                reqwest::StatusCode::OK,
                &json!({"data":{"status":"error","details":{"error":"InvalidCredentials"}}})
            ),
            Sent::Retry
        ));
        assert!(matches!(
            classify_expo(reqwest::StatusCode::TOO_MANY_REQUESTS, &json!({})),
            Sent::Retry
        ));
    }
}
