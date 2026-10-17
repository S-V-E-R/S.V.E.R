//! Direct messages (docs/COMMUNITY.md "Direct messages"): text between people who follow each
//! other (or whom the recipient follows, if they allow it), with stricter rules for under-18
//! accounts. Bodies are sealed with a DM-only purpose binding; staff can read only a reported
//! conversation's excerpt, through the report, and every read is audited.
//!
//! Private mode (opt-in per conversation): browsers encrypt to each other's device keys, and the
//! server stores only the ciphertext envelope and a commitment. Staff can never read one; a report
//! of a private message carries the reporter's plaintext, accepted only if it matches the
//! commitment.
use crate::{
    App,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
    routing::{delete, get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

const PURPOSE: &str = "dm";
const MAX_CHARS: usize = 1000;
const PAGE: i64 = 50;
const POLICIES: [&str; 3] = ["mutuals", "following", "nobody"];
/// Private mode: device keys per person, and the most a private envelope may hold.
const MAX_DEVICES: i64 = 5;
const MAX_ENVELOPE: usize = 12 * 1024;

type Facts = (bool, bool, bool, Option<String>, bool, bool, bool, bool);
/// Why `from` can't message `to`, or None when they can.
pub async fn refusal(db: &mut PgConnection, from: &str, to: &str) -> Res<Option<&'static str>> {
    if from == to {
        return Ok(Some("You can't message yourself."));
    }
    let (blocked, from_follows, to_follows, policy, to_adult, from_adult, from_verified, to_open): Facts = sqlx::query_as("SELECT
            EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1)),
            EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2),
            EXISTS(SELECT 1 FROM follows WHERE follower_id=$2 AND following_id=$1),
            (SELECT dm_policy FROM users WHERE id=$2),
            coalesce((SELECT date_of_birth<=current_date-interval '18 years' FROM users WHERE id=$2),false),
            coalesce((SELECT date_of_birth<=current_date-interval '18 years' FROM users WHERE id=$1),false),
            coalesce((SELECT email_verified FROM users WHERE id=$1),false),
            EXISTS(SELECT 1 FROM channel_users WHERE id=$2 AND eligible)")
        .bind(from).bind(to).fetch_one(db).await?;
    if blocked || !to_open {
        return Ok(Some("You can't message this person."));
    }
    if !from_verified {
        return Ok(Some("Verify your email to send messages."));
    }
    let mutual = from_follows && to_follows;
    // Under-18 accounts: always a mutual follow, and their own setting (off by default).
    if (!to_adult || !from_adult) && !mutual {
        return Ok(Some(
            "Messages with under-18 accounts need you to follow each other.",
        ));
    }
    let default = if to_adult { "mutuals" } else { "nobody" };
    Ok(match policy.as_deref().unwrap_or(default) {
        "nobody" => Some("They aren't accepting messages."),
        "following" if !to_follows => Some("They only accept messages from people they follow."),
        "mutuals" if !mutual => Some("You can message people who follow you back."),
        _ => None,
    })
}
async fn person(db: &mut PgConnection, name: &str) -> Res<profiles::ChannelUser> {
    profiles::eligible_by_name(db, name)
        .await?
        .ok_or_else(Fail::missing)
}
/// The conversation between two people, if there is one.
async fn conversation(db: &mut PgConnection, a: &str, b: &str) -> Res<Option<String>> {
    let (a, b) = if a < b { (a, b) } else { (b, a) };
    Ok(
        sqlx::query_scalar("SELECT id FROM dm_conversations WHERE user_a=$1 AND user_b=$2")
            .bind(a)
            .bind(b)
            .fetch_optional(db)
            .await?,
    )
}
fn open(app: &App, sealed: &str) -> String {
    sec::unseal(app, PURPOSE, sealed).unwrap_or_default()
}
type Row = (
    String,
    i64,
    String,
    String,
    DateTime<Utc>,
    Option<Value>,
    Option<String>,
);
/// The message columns every query selects (after the sender's username).
const COLUMNS: &str = "m.body_sealed,m.created_at,m.envelope,m.commitment";
fn message(app: &App, (id, seq, sender, sealed, at, envelope, commitment): &Row) -> Value {
    match envelope {
        Some(envelope) => {
            json!({"id": id, "seq": seq, "sender": sender, "body": null, "private": true, "envelope": envelope, "commitment": commitment, "created_at": at})
        }
        None => {
            json!({"id": id, "seq": seq, "sender": sender, "body": open(app, sealed), "private": false, "created_at": at})
        }
    }
}
const BLOCKED: &str = "EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=$2) OR (b.blocker_id=$2 AND b.blocked_id=$1))";

type Listed = (Value, Option<String>, Option<DateTime<Utc>>, i64, bool);
/// GET /api/dms: conversations, newest first, with the other person, the last message and unread.
async fn list(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let rows: Vec<Listed> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {chip},
            (SELECT m.body_sealed FROM dm_messages m WHERE m.conversation_id=c.id AND m.deleted_at IS NULL AND m.seq>s.cleared_seq ORDER BY m.seq DESC LIMIT 1),
            (SELECT m.created_at FROM dm_messages m WHERE m.conversation_id=c.id AND m.deleted_at IS NULL AND m.seq>s.cleared_seq ORDER BY m.seq DESC LIMIT 1),
            (SELECT count(*) FROM dm_messages m WHERE m.conversation_id=c.id AND m.deleted_at IS NULL AND m.sender_id<>$1 AND m.seq>greatest(s.read_seq,s.cleared_seq)),
            s.muted
        FROM dm_conversations c JOIN dm_state s ON s.conversation_id=c.id AND s.user_id=$1
        JOIN channel_users o ON o.id=CASE WHEN c.user_a=$1 THEN c.user_b ELSE c.user_a END AND o.eligible
        WHERE (c.user_a=$1 OR c.user_b=$1)
          AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=o.id) OR (b.blocker_id=o.id AND b.blocked_id=$1))
        ORDER BY c.last_message_at DESC LIMIT 100",
        chip = profiles::chip_sql("o")
    )))
    .bind(&me.id)
    .fetch_all(&app.db)
    .await?;
    let conversations: Vec<Value> = rows
        .into_iter()
        .filter(|r| r.1.is_some())
        .map(|(mut with, last, at, unread, muted)| {
            profiles::hydrate(&app, &mut with);
            // A private message's preview is just that: the server can't read it.
            let preview: String = match last.as_deref() {
                Some("") => "Private message".into(),
                Some(s) => open(&app, s).chars().take(80).collect(),
                None => String::new(),
            };
            json!({"with": with, "last": preview, "last_at": at, "unread": unread, "muted": muted})
        })
        .collect();
    Ok(Json(json!({"conversations": conversations})))
}
/// GET /api/dms/unread: unread messages in conversations that aren't muted (the header badge).
async fn unread(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM dm_messages m JOIN dm_state s ON s.conversation_id=m.conversation_id AND s.user_id=$1
        WHERE NOT s.muted AND m.deleted_at IS NULL AND m.sender_id<>$1 AND m.seq>greatest(s.read_seq,s.cleared_seq)
          AND NOT EXISTS(SELECT 1 FROM user_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=m.sender_id) OR (b.blocker_id=m.sender_id AND b.blocked_id=$1))")
        .bind(&me.id).fetch_one(&app.db).await?;
    Ok(Json(json!({"unread": count})))
}
#[derive(Deserialize)]
pub struct Before {
    before: Option<i64>,
}
/// GET /api/dms/{username}: the conversation (50 at a time, older with `before`), whether you can
/// send, and why not. A block hides the conversation for both people.
async fn read(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(page): Query<Before>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let other = person(&mut db, &name).await?;
    let mut with: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM channel_users c WHERE c.id=$1",
        profiles::chip_sql("c")
    )))
    .bind(&other.id)
    .fetch_one(&mut *db)
    .await?;
    profiles::hydrate(&app, &mut with);
    let why = refusal(&mut db, &me.id, &other.id).await?;
    let blocked: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {BLOCKED}")))
        .bind(&me.id)
        .bind(&other.id)
        .fetch_one(&mut *db)
        .await?;
    let id = conversation(&mut db, &me.id, &other.id).await?;
    let (private, changed): (bool, Value) = match &id {
        Some(id) => sqlx::query_as("SELECT c.private,CASE WHEN c.private_changed_at IS NULL THEN 'null'::jsonb ELSE jsonb_build_object('at',c.private_changed_at,'by',u.username) END FROM dm_conversations c LEFT JOIN users u ON u.id=c.private_changed_by WHERE c.id=$1")
            .bind(id)
            .fetch_one(&mut *db)
            .await?,
        None => (false, Value::Null),
    };
    // A new device on the other side is worth knowing about in a private conversation.
    let their_device_at: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT max(created_at) FROM dm_devices WHERE user_id=$1")
            .bind(&other.id)
            .fetch_one(&mut *db)
            .await?;
    let (messages, muted) = match id.filter(|_| !blocked) {
        None => (Vec::new(), false),
        Some(id) => {
            let muted: bool = sqlx::query_scalar(
                "SELECT muted FROM dm_state WHERE conversation_id=$1 AND user_id=$2",
            )
            .bind(&id)
            .bind(&me.id)
            .fetch_optional(&mut *db)
            .await?
            .unwrap_or(false);
            let mut rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT m.id,m.seq,u.username,{COLUMNS} FROM dm_messages m JOIN users u ON u.id=m.sender_id
                WHERE m.conversation_id=$1 AND m.deleted_at IS NULL AND m.seq>coalesce((SELECT cleared_seq FROM dm_state WHERE conversation_id=$1 AND user_id=$2),0)
                  AND ($3::bigint IS NULL OR m.seq<$3) ORDER BY m.seq DESC LIMIT $4")))
                .bind(&id).bind(&me.id).bind(page.before).bind(PAGE).fetch_all(&mut *db).await?;
            rows.reverse();
            (rows.iter().map(|r| message(&app, r)).collect(), muted)
        }
    };
    Ok(Json(
        json!({"with": with, "messages": messages, "can_send": why.is_none(), "reason": why, "muted": muted, "private": private, "private_changed": changed, "their_device_at": their_device_at}),
    ))
}
#[derive(Deserialize)]
pub struct Send {
    #[serde(default)]
    body: String,
    /// Private mode: the ciphertext envelope and its commitment, instead of a body.
    #[serde(default)]
    envelope: Option<Value>,
    #[serde(default)]
    commitment: Option<String>,
}
/// The franking commitment: SHA-256 of the 32 secret bytes then the text, as lowercase hex.
pub fn franking_hex(frank: &[u8], body: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest([frank, body.as_bytes()].concat())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn b64(value: &Value) -> Option<Vec<u8>> {
    value.as_str().and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
}
/// A device's public key: an uncompressed P-256 point (65 bytes starting 0x04), base64url.
fn valid_key(key: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(key)
        .is_ok_and(|k| k.len() == 65 && k[0] == 4)
}
/// Checks a private message's envelope: its shape and sizes, and that every wrapped key is for a
/// device of one of the two people, with at least one of the recipient's.
async fn check_envelope(
    db: &mut PgConnection,
    envelope: &Value,
    me: &str,
    other: &str,
) -> Res<Value> {
    let bad = || {
        Fail::field(
            "envelope",
            "That private message couldn't be sent. Reload and try again.",
        )
    };
    if envelope["v"] != 1 || envelope.to_string().len() > MAX_ENVELOPE {
        return Err(bad());
    }
    let epk = b64(&envelope["epk"]).ok_or_else(bad)?;
    let iv = b64(&envelope["iv"]).ok_or_else(bad)?;
    let ct = b64(&envelope["ct"]).ok_or_else(bad)?;
    // The plaintext is {"body": ≤1,000 characters, "frank": 43 characters} as JSON.
    if epk.len() != 65 || epk[0] != 4 || iv.len() != 12 || !(17..=6000).contains(&ct.len()) {
        return Err(bad());
    }
    let keys = envelope["keys"].as_object().ok_or_else(bad)?;
    if keys.is_empty() || keys.len() > 2 * MAX_DEVICES as usize {
        return Err(bad());
    }
    let devices: Vec<(String, String)> =
        sqlx::query_as("SELECT id,user_id FROM dm_devices WHERE user_id IN ($1,$2)")
            .bind(me)
            .bind(other)
            .fetch_all(db)
            .await?;
    let mut theirs = false;
    let mut kept = serde_json::Map::new();
    for (device, wrapped) in keys {
        let owner = devices
            .iter()
            .find(|(id, _)| id == device)
            .map(|(_, u)| u)
            .ok_or_else(bad)?;
        let (iv, key) = (b64(&wrapped["iv"]), b64(&wrapped["k"]));
        if iv.is_none_or(|v| v.len() != 12) || key.is_none_or(|k| k.len() != 48) {
            return Err(bad());
        }
        theirs |= owner == other;
        kept.insert(
            device.clone(),
            json!({"iv": wrapped["iv"], "k": wrapped["k"]}),
        );
    }
    if !theirs {
        return Err(bad());
    }
    // Only the checked fields are stored and served back.
    Ok(
        json!({"v": 1, "epk": envelope["epk"], "iv": envelope["iv"], "ct": envelope["ct"], "keys": kept}),
    )
}
/// POST /api/dms/{username}: sends a message.
async fn send(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    crate::switches::guard(&mut *app.db.acquire().await?, "dms").await?;
    let body = input.body.trim();
    let private = input.envelope.is_some();
    if private {
        let ok = input.commitment.as_deref().is_some_and(|c| {
            c.len() == 64
                && c.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if !ok || !body.is_empty() {
            return Err(Fail::field(
                "envelope",
                "That private message couldn't be sent. Reload and try again.",
            ));
        }
    } else if body.is_empty()
        || body.chars().count() > MAX_CHARS
        || body.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(Fail::field(
            "body",
            "Write a message of 1–1,000 characters.",
        ));
    }
    profiles::rate(&app, format!("dm:{}", me.id), 30, 60).await?;
    let mut tx = app.db.begin().await?;
    profiles::ensure_unrestricted(&mut tx, &me.id).await?;
    let other = person(&mut tx, &name).await?;
    if let Some(why) = refusal(&mut tx, &me.id, &other.id).await? {
        return Err(Fail::denied(why));
    }
    let (a, b) = if me.id < other.id {
        (&me.id, &other.id)
    } else {
        (&other.id, &me.id)
    };
    let id: String = sqlx::query_scalar("INSERT INTO dm_conversations(id,user_a,user_b) VALUES($1,$2,$3) ON CONFLICT(user_a,user_b) DO UPDATE SET last_message_at=now() RETURNING id")
        .bind(profiles::new_id()).bind(a).bind(b).fetch_one(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO dm_state(conversation_id,user_id) VALUES($1,$2),($1,$3) ON CONFLICT DO NOTHING",
    )
    .bind(&id)
    .bind(&me.id)
    .bind(&other.id)
    .execute(&mut *tx)
    .await?;
    // In Private mode only envelopes go through, and envelopes only in Private mode.
    let is_private: bool = sqlx::query_scalar("SELECT private FROM dm_conversations WHERE id=$1")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    if is_private != private {
        return Err(Fail::conflict(if is_private {
            "This conversation is private now. Reload to send an encrypted message."
        } else {
            "This conversation isn't private."
        }));
    }
    let envelope = match &input.envelope {
        Some(envelope) => Some(check_envelope(&mut tx, envelope, &me.id, &other.id).await?),
        None => None,
    };
    // L4: a commitment goes only with an envelope.
    let commitment = input.commitment.as_ref().filter(|_| private);
    let sealed = if private {
        String::new()
    } else {
        sec::seal(&app, PURPOSE, body)?
    };
    let row: Row = sqlx::query_as("INSERT INTO dm_messages(id,conversation_id,sender_id,body_sealed,envelope,commitment) VALUES($1,$2,$3,$4,$6,$7) RETURNING id,seq,$5,body_sealed,created_at,envelope,commitment")
        .bind(profiles::new_id()).bind(&id).bind(&me.id).bind(sealed).bind(&me.username).bind(&envelope).bind(commitment)
        .fetch_one(&mut *tx).await?;
    // The recipient hears about the first unread message of a streak (never the text, which is
    // sealed), unless they muted the conversation; their notification settings decide site/push.
    // Unread as the recipient sees it (the list and the badge): after what they've read or
    // deleted for themselves, and not counting messages staff removed.
    let (unread, muted): (i64, bool) = sqlx::query_as("SELECT (SELECT count(*) FROM dm_messages m WHERE m.conversation_id=$1 AND m.sender_id<>$2 AND m.deleted_at IS NULL AND m.seq>greatest(s.read_seq,s.cleared_seq)),s.muted FROM dm_state s WHERE s.conversation_id=$1 AND s.user_id=$2")
        .bind(&id).bind(&other.id).fetch_one(&mut *tx).await?;
    if unread == 1 && !muted {
        crate::alerts::community(&mut tx, &other.id, &me.id, "dm", &format!("{id}:{}", row.1), None,
            &json!({"title": "New message", "body": format!("{} sent you a message.", me.username), "url": format!("/messages?with={}", me.username)})).await?;
    }
    // Sending reads everything before it.
    sqlx::query("UPDATE dm_state SET read_seq=$3 WHERE conversation_id=$1 AND user_id=$2")
        .bind(&id)
        .bind(&me.id)
        .bind(row.1)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let message = message(&app, &row);
    for (to, with) in [(&other.id, &me.username), (&me.id, &other.username)] {
        app.dms.publish(
            &format!("dm:{to}"),
            None,
            row.1,
            json!({"type": "dm", "with": with, "message": message}),
        );
    }
    Ok(Json(json!({"message": message})))
}
async fn state_for(db: &mut PgConnection, me: &str, name: &str) -> Res<String> {
    let other = person(db, name).await?;
    conversation(db, me, &other.id)
        .await?
        .ok_or_else(Fail::missing)
}
/// POST /api/dms/{username}/read
async fn mark_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let id = state_for(&mut db, &me.id, &name).await?;
    sqlx::query("UPDATE dm_state SET read_seq=greatest(read_seq,coalesce((SELECT max(seq) FROM dm_messages WHERE conversation_id=$1),0)) WHERE conversation_id=$1 AND user_id=$2")
        .bind(&id).bind(&me.id).execute(&mut *db).await?;
    Ok(Json(json!({"read": true})))
}
#[derive(Deserialize)]
pub struct Mute {
    muted: bool,
}
/// PUT /api/dms/{username}/mute: no unread badge for this conversation.
async fn mute(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Mute>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let id = state_for(&mut db, &me.id, &name).await?;
    sqlx::query("UPDATE dm_state SET muted=$3 WHERE conversation_id=$1 AND user_id=$2")
        .bind(&id)
        .bind(&me.id)
        .bind(input.muted)
        .execute(&mut *db)
        .await?;
    Ok(Json(json!({"muted": input.muted})))
}
/// DELETE /api/dms/{username}: removes the conversation from your own view only.
async fn clear(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let id = state_for(&mut db, &me.id, &name).await?;
    sqlx::query("UPDATE dm_state SET cleared_seq=coalesce((SELECT max(seq) FROM dm_messages WHERE conversation_id=$1),0) WHERE conversation_id=$1 AND user_id=$2")
        .bind(&id).bind(&me.id).execute(&mut *db).await?;
    Ok(Json(json!({"cleared": true})))
}
#[derive(Deserialize)]
pub struct Settings {
    policy: String,
}
/// GET /api/me/dm-settings: who can message you (the effective default for your age).
async fn settings(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let (policy, adult): (Option<String>, bool) = sqlx::query_as("SELECT dm_policy,coalesce(date_of_birth<=current_date-interval '18 years',false) FROM users WHERE id=$1")
        .bind(&me.id).fetch_one(&app.db).await?;
    let policy = policy.unwrap_or_else(|| if adult { "mutuals" } else { "nobody" }.into());
    Ok(Json(json!({"policy": policy, "adult": adult})))
}
/// PUT /api/me/dm-settings
async fn set_settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if !POLICIES.contains(&input.policy.as_str()) {
        return Err(Fail::field("policy", "Choose who can message you."));
    }
    sqlx::query("UPDATE users SET dm_policy=$2 WHERE id=$1")
        .bind(&me.id)
        .bind(&input.policy)
        .execute(&app.db)
        .await?;
    settings(State(app), jar).await
}

/// GET /api/dms/socket: new messages for the signed-in person, as they arrive.
async fn socket(
    State(app): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let me = profiles::signed_in(&app, &jar).await?;
    Ok(upgrade
        .max_message_size(4096)
        .on_upgrade(move |ws| session(app, jar, me.id, ws)))
}
/// Whether the socket's session still stands, by the rules `auth::session` applies (not revoked or
/// expired, same auth version, 2FA done, account not being deleted), without renewing it.
async fn still_signed_in(app: &App, jar: &CookieJar, me: &str) -> bool {
    let Some(token) = jar.get(app.config.cookie_name()) else {
        return false;
    };
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.expires_at>now()
        AND s.auth_version=u.auth_version AND (s.mfa_verified OR NOT u.mfa_enabled) AND u.deleted_at IS NULL AND NOT u.legacy_deletion_hold)")
        .bind(sec::digest(token.value()))
        .bind(me)
        .fetch_one(&app.db)
        .await
        .unwrap_or(false)
}
/// Message text goes out on this socket, so it ends with the session (signing out everywhere, a
/// password change, account deletion): checked before every delivery and each minute while idle.
async fn session(app: App, jar: CookieJar, me: String, mut ws: WebSocket) {
    let mut events = app.dms.subscribe();
    let room = format!("dm:{me}");
    let signed_out = Message::Close(Some(axum::extract::ws::CloseFrame {
        code: 4001,
        reason: "signed out".into(),
    }));
    let mut idle = tokio::time::interval(std::time::Duration::from_secs(60));
    idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    idle.tick().await;
    loop {
        tokio::select! {
            _ = idle.tick() => {
                if !still_signed_in(&app, &jar, &me).await {
                    let _ = ws.send(signed_out).await;
                    return;
                }
            }
            event = events.recv() => match event {
                Ok(e) if e.channel == room => {
                    if !still_signed_in(&app, &jar, &me).await {
                        let _ = ws.send(signed_out).await;
                        return;
                    }
                    if ws.send(Message::Text(e.payload.to_string().into())).await.is_err() { return; }
                }
                Ok(_) => {}
                // Missed events: the client reloads the conversation.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if ws.send(Message::Text(json!({"type":"resync"}).to_string().into())).await.is_err() { return; }
                }
                Err(_) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

// ---- Private mode ----

/// GET /api/me/dm-devices: this account's device keys.
async fn devices(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    device_list(&app, &me.id).await
}
async fn device_list(app: &App, user: &str) -> Res<Json<Value>> {
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'public_key',public_key,'created_at',created_at) FROM dm_devices WHERE user_id=$1 ORDER BY created_at")
        .bind(user).fetch_all(&app.db).await?;
    Ok(Json(json!({"devices": rows, "max": MAX_DEVICES})))
}
#[derive(Deserialize)]
pub struct NewDevice {
    public_key: String,
}
/// POST /api/me/dm-devices: registers this browser's public key (up to 5 devices).
async fn add_device(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewDevice>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    if !valid_key(&input.public_key) {
        return Err(Fail::field("public_key", "That device key isn't valid."));
    }
    profiles::rate(&app, format!("dm-device:{}", me.id), 10, 3600).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,94))")
        .bind(&me.id)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM dm_devices WHERE user_id=$1")
        .bind(&me.id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_DEVICES {
        return Err(Fail::conflict(
            "You have 5 devices with private messages. Remove one first.",
        ));
    }
    let id = profiles::new_id();
    sqlx::query("INSERT INTO dm_devices(id,user_id,public_key) VALUES($1,$2,$3)")
        .bind(&id)
        .bind(&me.id)
        .bind(&input.public_key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut list = device_list(&app, &me.id).await?.0;
    list["id"] = json!(id);
    Ok(Json(list))
}
/// DELETE /api/me/dm-devices/{id}: that device can no longer read new private messages.
async fn remove_device(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM dm_devices WHERE id=$1 AND user_id=$2")
        .bind(&id)
        .bind(&me.id)
        .execute(&app.db)
        .await?;
    device_list(&app, &me.id).await
}
/// GET /api/dms/{username}/keys: both people's device keys, for encrypting to them.
async fn keys(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let other = person(&mut db, &name).await?;
    let blocked: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {BLOCKED}")))
        .bind(&me.id)
        .bind(&other.id)
        .fetch_one(&mut *db)
        .await?;
    let talking = conversation(&mut db, &me.id, &other.id).await?.is_some();
    if blocked || (!talking && refusal(&mut db, &me.id, &other.id).await?.is_some()) {
        return Err(Fail::missing());
    }
    let mut lists = Vec::new();
    for user in [&me.id, &other.id] {
        let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'public_key',public_key) FROM dm_devices WHERE user_id=$1 ORDER BY created_at")
            .bind(user).fetch_all(&mut *db).await?;
        lists.push(rows);
    }
    Ok(Json(json!({"mine": lists[0], "theirs": lists[1]})))
}
#[derive(Deserialize)]
pub struct Private {
    on: bool,
}
/// PUT /api/dms/{username}/private: either person turns Private mode on (both need a device key)
/// or off. Both see the state in the conversation.
async fn set_private(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Private>,
) -> Res<Json<Value>> {
    let me = profiles::signed_in(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let other = person(&mut tx, &name).await?;
    if let Some(why) = refusal(&mut tx, &me.id, &other.id).await? {
        return Err(Fail::denied(why));
    }
    if input.on {
        let adults: bool = sqlx::query_scalar("SELECT count(*)=2 FROM users WHERE id IN ($1,$2) AND date_of_birth<=current_date-interval '18 years'")
            .bind(&me.id).bind(&other.id).fetch_one(&mut *tx).await?;
        if !adults {
            return Err(Fail::denied(
                "Private mode is for conversations between two adults.",
            ));
        }
        let ready: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM dm_devices WHERE user_id=$1) AND EXISTS(SELECT 1 FROM dm_devices WHERE user_id=$2)")
            .bind(&me.id).bind(&other.id).fetch_one(&mut *tx).await?;
        if !ready {
            return Err(Fail::conflict(
                "Both of you need to open Messages on a device that supports Private mode first.",
            ));
        }
    }
    let (a, b) = if me.id < other.id {
        (&me.id, &other.id)
    } else {
        (&other.id, &me.id)
    };
    let id: String = sqlx::query_scalar("INSERT INTO dm_conversations(id,user_a,user_b,private,private_changed_at,private_changed_by) VALUES($1,$2,$3,$4,now(),$5) ON CONFLICT(user_a,user_b) DO UPDATE SET private=EXCLUDED.private,private_changed_at=now(),private_changed_by=EXCLUDED.private_changed_by RETURNING id")
        .bind(profiles::new_id()).bind(a).bind(b).bind(input.on).bind(&me.id).fetch_one(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO dm_state(conversation_id,user_id) VALUES($1,$2),($1,$3) ON CONFLICT DO NOTHING",
    )
    .bind(&id)
    .bind(&me.id)
    .bind(&other.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    for (to, with) in [(&other.id, &me.username), (&me.id, &other.username)] {
        app.dms.publish(
            &format!("dm:{to}"),
            None,
            0,
            json!({"type": "private", "with": with, "private": input.on}),
        );
    }
    Ok(Json(json!({"private": input.on})))
}

// ---- Reports and expiry ----

/// A DM report's target: the reporter must be the other person in the conversation. The snapshot
/// keeps the reported message and up to 20 before it, sealed; staff open it through `staff_read`.
pub async fn report_target(
    app: &App,
    db: &mut PgConnection,
    id: &str,
    reporter: &str,
    evidence: Option<&Value>,
) -> Res<(String, String, Value)> {
    let found: Option<(String, String, String)> = sqlx::query_as("SELECT m.conversation_id,m.sender_id,u.username FROM dm_messages m JOIN dm_conversations c ON c.id=m.conversation_id JOIN users u ON u.id=m.sender_id
        WHERE m.id=$1 AND m.deleted_at IS NULL AND (c.user_a=$2 OR c.user_b=$2) AND m.sender_id<>$2")
        .bind(id).bind(reporter).fetch_optional(&mut *db).await?;
    let (conversation, sender, sender_name) = found.ok_or_else(Fail::missing)?;
    let commitment: Option<String> =
        sqlx::query_scalar("SELECT commitment FROM dm_messages WHERE id=$1")
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
    let excerpt: Vec<Value> = match commitment {
        // A private message: the reporter's plaintext, accepted only if it matches what the sender
        // committed to. Nothing else in a private conversation can be read by anyone but the two.
        Some(_) if evidence.is_none() => {
            let at: DateTime<Utc> =
                sqlx::query_scalar("SELECT created_at FROM dm_messages WHERE id=$1")
                    .bind(id)
                    .fetch_one(&mut *db)
                    .await?;
            vec![
                json!({"id": id, "sender": sender_name, "body": null, "private": true, "unverifiable": true, "created_at": at}),
            ]
        }
        Some(commitment) => {
            let body = evidence
                .and_then(|e| e["body"].as_str())
                .unwrap_or_default();
            let frank = evidence.and_then(|e| b64(&e["frank"])).unwrap_or_default();
            if frank.len() != 32 || franking_hex(&frank, body) != commitment {
                return Err(Fail::field(
                    "evidence",
                    "This private message couldn't be verified for a report.",
                ));
            }
            let at: DateTime<Utc> =
                sqlx::query_scalar("SELECT created_at FROM dm_messages WHERE id=$1")
                    .bind(id)
                    .fetch_one(&mut *db)
                    .await?;
            vec![
                json!({"id": id, "sender": sender_name, "body": body, "private": true, "verified": true, "created_at": at}),
            ]
        }
        None => {
            let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT m.id,m.seq,u.username,{COLUMNS} FROM dm_messages m JOIN users u ON u.id=m.sender_id
                WHERE m.conversation_id=$1 AND m.seq<=(SELECT seq FROM dm_messages WHERE id=$2) ORDER BY m.seq DESC LIMIT 21")))
                .bind(&conversation).bind(id).fetch_all(&mut *db).await?;
            // Earlier private messages stay private: only their place in the excerpt shows.
            rows.iter()
                .rev()
                .map(|r| {
                    let mut m = message(app, r);
                    if let Some(o) = m.as_object_mut() {
                        o.remove("envelope");
                        o.remove("commitment");
                    }
                    m
                })
                .collect()
        }
    };

    let sealed = sec::seal(
        app,
        "dm-report",
        &json!({"reported": id, "messages": excerpt}).to_string(),
    )?;
    Ok((
        sender,
        sender_name,
        json!({"sealed": sealed, "messages": excerpt.len()}),
    ))
}
/// GET /api/admin/dm-reports/{report}: staff open a reported conversation excerpt. Audited.
async fn staff_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(report): Path<String>,
) -> Res<Json<Value>> {
    let staff = crate::safety::staff(&app, &jar).await?;
    let mut db = app.db.acquire().await?;
    let (target, snapshot): (String, Value) = sqlx::query_as(
        "SELECT target_id,snapshot FROM reports WHERE id=$1 AND target_type='dm_message'",
    )
    .bind(&report)
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(Fail::missing)?;
    let sealed = snapshot["value"]["sealed"]
        .as_str()
        .ok_or_else(Fail::missing)?;
    let excerpt: Value = serde_json::from_str(&sec::unseal(&app, "dm-report", sealed)?)
        .map_err(|_| Fail::missing())?;
    crate::safety::audit(
        &mut db,
        Some(&staff.id),
        "dm_read",
        "dm_message",
        &target,
        std::slice::from_ref(&report),
        "Read a reported conversation",
        json!({}),
        false,
    )
    .await?;
    Ok(Json(excerpt))
}
/// Hides a reported message (a staff removal); true when it was visible.
pub async fn remove(db: &mut PgConnection, id: &str) -> Res<bool> {
    let visible: bool =
        sqlx::query_scalar("SELECT deleted_at IS NULL FROM dm_messages WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(Fail::missing)?;
    if visible {
        sqlx::query("UPDATE dm_messages SET deleted_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *db)
            .await?;
    }
    Ok(visible)
}
/// Expired messages are deleted unless an open report holds them; emptied old conversations go.
pub async fn tick(app: &App) -> Res<()> {
    sqlx::query("DELETE FROM dm_messages m WHERE m.expires_at<now() AND NOT EXISTS(SELECT 1 FROM reports r WHERE r.target_type='dm_message' AND r.target_id=m.id AND r.status='OPEN')")
        .execute(&app.db).await?;
    sqlx::query("DELETE FROM dm_conversations c WHERE c.last_message_at<now()-interval '12 months' AND NOT EXISTS(SELECT 1 FROM dm_messages m WHERE m.conversation_id=c.id)")
        .execute(&app.db).await?;
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/dms", get(list))
        .route("/api/dms/unread", get(unread))
        .route("/api/dms/socket", get(socket))
        .route("/api/dms/{username}", get(read).post(send).delete(clear))
        .route("/api/dms/{username}/read", post(mark_read))
        .route("/api/dms/{username}/mute", put(mute))
        .route("/api/me/dm-settings", get(settings).put(set_settings))
        .route("/api/me/dm-devices", get(devices).post(add_device))
        .route("/api/me/dm-devices/{id}", delete(remove_device))
        .route("/api/dms/{username}/keys", get(keys))
        .route("/api/dms/{username}/private", put(set_private))
        .route("/api/admin/dm-reports/{report}", get(staff_read))
}
