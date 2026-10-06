//! Module 7 CrowdSync, phase 1 (docs/CROWDSYNC.md "Boards"): streamer-built boards of controls
//! under the player. A press by a verified viewer on a counted or trusted playback session spends
//! the channel's Engagement Valor and is recorded, with its goal progress and webhook delivery, in
//! one transaction; effects go out on the chat hub carrying stream time, so each player shows them
//! when its own video reaches that moment. Webhooks leave through the Postgres outbox.
use crate::{
    App, auth,
    chat::Event,
    moderation::{self, Role},
    profiles::{self, Fail, Res},
    security as sec, stripe, text,
};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, DefaultBodyLimit, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
    routing::{get, post, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

/// Preset on-stream effects (the S.V.E.R library). Streamer uploads and sounds come with Skills.
pub const EFFECTS: &[&str] = &[
    "none",
    "confetti",
    "hearts",
    "fireworks",
    "stars",
    "rain",
    "shake",
    "spotlight",
];
const KINDS: &[&str] = &["button", "label", "text", "goal", "joystick"];
const AUDIENCES: &[&str] = &["everyone", "followers", "subscribers", "moderators"];
const MAX_SCREENS: usize = 4;
const MAX_CONTROLS: usize = 24;
/// Text inputs (**Proposed** in the spec).
const MAX_TEXT: usize = 200;
/// Webhook attempts before giving up (backoff doubles from 10 seconds).
const MAX_ATTEMPTS: i32 = 8;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub screens: Vec<Screen>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub name: String,
    #[serde(default)]
    pub controls: Vec<Control>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub id: String,
    pub kind: String,
    pub label: String,
    /// Engagement Valor per press; 0 is free.
    #[serde(default)]
    pub cost: i32,
    /// Per viewer.
    #[serde(default)]
    pub cooldown_seconds: i32,
    /// Presses by everyone during one stream.
    #[serde(default)]
    pub per_stream_limit: Option<i32>,
    #[serde(default = "everyone")]
    pub audience: String,
    #[serde(default = "no_effect")]
    pub effect: String,
    /// Goals only: progress needed (each press adds its cost, or 1 when free).
    #[serde(default)]
    pub target: Option<i32>,
    /// Grid columns (of 4).
    #[serde(default = "one")]
    pub width: u8,
}
fn everyone() -> String {
    "everyone".into()
}
fn no_effect() -> String {
    "none".into()
}
fn one() -> u8 {
    1
}
impl Board {
    fn controls(&self) -> impl Iterator<Item = &Control> {
        self.screens.iter().flat_map(|s| s.controls.iter())
    }
    fn control(&self, id: &str) -> Option<&Control> {
        self.controls().find(|c| c.id == id)
    }
    fn empty() -> Self {
        Board {
            screens: vec![Screen {
                name: "Main".into(),
                controls: vec![],
            }],
        }
    }
}

fn bad(control: &Control, message: &str) -> Fail {
    Fail::bad(format!("{}: {message}", control.label.trim()))
}
/// Every rule the builder shows, checked again here.
pub fn validate(board: &Board) -> Res<()> {
    if !(1..=MAX_SCREENS).contains(&board.screens.len()) {
        return Err(Fail::bad("A board has 1 to 4 screens."));
    }
    let mut ids = HashSet::new();
    for screen in &board.screens {
        let name = screen.name.trim();
        if !(1..=30).contains(&name.chars().count()) {
            return Err(Fail::bad("Screen names are 1–30 characters."));
        }
        text::filter(name, "name")?;
        if screen.controls.len() > MAX_CONTROLS {
            return Err(Fail::bad("A screen has up to 24 controls."));
        }
        for c in &screen.controls {
            if !(1..=24).contains(&c.id.len())
                || !c
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || !ids.insert(c.id.as_str())
            {
                return Err(Fail::bad("Each control needs its own short ID."));
            }
            let label = c.label.trim();
            if !(1..=40).contains(&label.chars().count()) {
                return Err(Fail::bad("Control labels are 1–40 characters."));
            }
            text::filter(label, "label")?;
            if !KINDS.contains(&c.kind.as_str()) {
                return Err(bad(c, "unknown control type."));
            }
            if !AUDIENCES.contains(&c.audience.as_str()) {
                return Err(bad(c, "choose who can use it."));
            }
            if !EFFECTS.contains(&c.effect.as_str()) {
                return Err(bad(c, "choose an effect from the library."));
            }
            if !(0..=100_000).contains(&c.cost) {
                return Err(bad(c, "cost is 0 to 100,000 Engagement Valor."));
            }
            if !(0..=3600).contains(&c.cooldown_seconds) {
                return Err(bad(c, "cooldown is up to an hour."));
            }
            if c.per_stream_limit.is_some_and(|l| !(1..=1000).contains(&l)) {
                return Err(bad(c, "the per-stream limit is 1 to 1,000."));
            }
            if !(1..=4).contains(&c.width) {
                return Err(bad(c, "width is 1 to 4 columns."));
            }
            let simple = c.cost == 0
                && c.cooldown_seconds == 0
                && c.per_stream_limit.is_none()
                && c.effect == "none";
            match c.kind.as_str() {
                "label" if !simple => return Err(bad(c, "labels can't be pressed.")),
                "joystick" if !simple => {
                    return Err(bad(c, "joysticks are free and have no effect or limits."));
                }
                "goal" if !c.target.is_some_and(|t| (1..=1_000_000).contains(&t)) => {
                    return Err(bad(c, "set a goal of 1 to 1,000,000."));
                }
                kind if kind != "goal" && c.target.is_some() => {
                    return Err(bad(c, "only goals have a target."));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn pressable(c: &Control) -> bool {
    c.kind != "label"
}

/// Starting points in the builder.
pub fn templates() -> Vec<(&'static str, Board)> {
    let c = |id: &str, kind: &str, label: &str, cost: i32, effect: &str| Control {
        id: id.into(),
        kind: kind.into(),
        label: label.into(),
        cost,
        cooldown_seconds: if kind == "joystick" || kind == "label" {
            0
        } else {
            30
        },
        per_stream_limit: None,
        audience: "everyone".into(),
        effect: effect.into(),
        target: None,
        width: if kind == "label" { 4 } else { 1 },
    };
    let screen = |name: &str, controls: Vec<Control>| Board {
        screens: vec![Screen {
            name: name.into(),
            controls,
        }],
    };
    let mut goal = c("goal", "goal", "Fill the hype bar", 10, "fireworks");
    goal.target = Some(1000);
    goal.width = 4;
    let mut stick = c("move", "joystick", "Move", 0, "none");
    stick.width = 2;
    vec![
        (
            "Hype buttons",
            screen(
                "Hype",
                vec![
                    c("confetti", "button", "Confetti", 50, "confetti"),
                    c("hearts", "button", "Hearts", 25, "hearts"),
                    c("fireworks", "button", "Fireworks", 100, "fireworks"),
                    c("stars", "button", "Stars", 0, "stars"),
                ],
            ),
        ),
        (
            "Community goal",
            screen(
                "Goal",
                vec![
                    c(
                        "about",
                        "label",
                        "Fill the bar together for fireworks!",
                        0,
                        "none",
                    ),
                    goal,
                    c("cheer", "button", "Cheer", 0, "hearts"),
                ],
            ),
        ),
        (
            "Shout-outs",
            screen(
                "Shout-outs",
                vec![
                    c("about", "label", "Your message shows on stream.", 0, "none"),
                    c("say", "text", "Say something", 100, "spotlight"),
                ],
            ),
        ),
        (
            "Arcade",
            screen(
                "Arcade",
                vec![
                    stick,
                    c("a", "button", "A", 0, "none"),
                    c("b", "button", "B", 0, "none"),
                ],
            ),
        ),
    ]
}

/// Whether the channel has a published board (the channel page shows the panel's opener).
pub async fn published(db: &mut sqlx::PgConnection, channel: &str) -> Res<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM boards WHERE channel_id=$1 AND published IS NOT NULL)",
    )
    .bind(channel)
    .fetch_one(db)
    .await?)
}

#[derive(sqlx::FromRow)]
struct Row {
    draft: sqlx::types::Json<Board>,
    published: Option<sqlx::types::Json<Board>>,
    version: i32,
    published_at: Option<DateTime<Utc>>,
    disabled: bool,
    moderators_run: bool,
    /// The OBS overlay checked in within the last 30 seconds.
    overlay: bool,
    overlay_set: bool,
    webhook_url: Option<String>,
}
const ROW: &str = "SELECT draft,published,version,published_at,disabled,moderators_run,
    coalesce(overlay_seen_at>now()-interval '30 seconds',false) AS overlay, overlay_token_hash IS NOT NULL AS overlay_set, webhook_url FROM boards";
async fn row(app: &App, channel: &str) -> Res<Option<Row>> {
    // ROW is fixed SQL; the channel is bound.
    Ok(
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{ROW} WHERE channel_id=$1")))
            .bind(channel)
            .fetch_optional(&app.db)
            .await?,
    )
}
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}
/// The channel's live broadcast: (id, milliseconds since it started).
async fn live(app: &App, channel: &str) -> Res<Option<(String, i64)>> {
    Ok(sqlx::query_as("SELECT id,(extract(epoch FROM now()-started_at)*1000)::bigint FROM broadcasts WHERE owner_id=$1 AND state IN ('LIVE','RECONNECTING') AND started_at IS NOT NULL")
        .bind(channel)
        .fetch_optional(&app.db)
        .await?)
}
fn checklist(row: &Row) -> Vec<Value> {
    let draft = &row.draft.0;
    let usable = draft.controls().any(pressable);
    let changed = row.published.as_ref().is_none_or(|p| p.0 != *draft);
    vec![
        json!({"label": "At least one control viewers can use", "ok": usable, "required": true}),
        json!({"label": "Changes since the last publish", "ok": changed, "required": true}),
        json!({"label": "OBS overlay connected (otherwise effects show over the player)", "ok": row.overlay, "required": false}),
    ]
}

// ---- Creator Studio ----

/// GET /api/me/board
async fn mine(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    studio(&app, &user).await.map(Json)
}
async fn studio(app: &App, user: &auth::User) -> Res<Value> {
    let row = match row(app, &user.id).await? {
        Some(row) => row,
        None => Row {
            draft: sqlx::types::Json(Board::empty()),
            published: None,
            version: 0,
            published_at: None,
            disabled: false,
            moderators_run: false,
            overlay: false,
            overlay_set: false,
            webhook_url: None,
        },
    };
    let templates: Vec<Value> = templates()
        .into_iter()
        .map(|(name, board)| json!({"name": name, "board": board}))
        .collect();
    Ok(json!({
        "username": user.username,
        "draft": row.draft.0, "published": row.published.as_ref().map(|p| &p.0),
        "version": row.version, "published_at": row.published_at,
        "disabled": row.disabled, "moderators_run": row.moderators_run,
        "overlay": {"set": row.overlay_set, "connected": row.overlay},
        "webhook_url": row.webhook_url,
        "checklist": checklist(&row),
        "templates": templates, "effects": EFFECTS, "kinds": KINDS, "audiences": AUDIENCES,
    }))
}

#[derive(Deserialize)]
pub struct Draft {
    board: Board,
}
/// PUT /api/me/board/draft: editing never touches the live board.
async fn save(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Draft>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    validate(&input.board)?;
    sqlx::query("INSERT INTO boards(channel_id,draft) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET draft=EXCLUDED.draft, updated_at=now()")
        .bind(&user.id)
        .bind(sqlx::types::Json(&input.board))
        .execute(&app.db)
        .await?;
    studio(&app, &user).await.map(Json)
}

/// POST /api/me/board/publish: the draft goes live as a new version; goals start over.
async fn publish(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let row = row(&app, &user.id)
        .await?
        .ok_or_else(|| Fail::bad("Build a board first."))?;
    validate(&row.draft.0)?;
    if let Some(missing) = checklist(&row)
        .iter()
        .find(|c| c["required"] == true && c["ok"] == false)
    {
        return Err(Fail::bad(format!(
            "Before publishing: {}.",
            missing["label"].as_str().unwrap_or_default()
        )));
    }
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE boards SET published=draft, version=version+1, published_at=now() WHERE channel_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM board_goals WHERE channel_id=$1")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    changed(&app, &user.id);
    studio(&app, &user).await.map(Json)
}
/// Open boards and overlays reload.
fn changed(app: &App, channel: &str) {
    app.chat.publish(channel, None, 0, json!({"type": "board"}));
}

#[derive(Deserialize)]
pub struct Test {
    control: String,
}
/// POST /api/me/board/test: plays a draft control's effect in the Studio preview and the
/// streamer's OBS overlay only. Nothing is charged or recorded, and viewers see nothing.
async fn test(State(app): State<App>, jar: CookieJar, Json(input): Json<Test>) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sec::reserve(&app, vec![format!("board-test:{}", user.id)], 30, 60).await?;
    let row = row(&app, &user.id).await?.ok_or_else(Fail::missing)?;
    let control = row
        .draft
        .0
        .control(&input.control)
        .ok_or_else(Fail::missing)?;
    let effect = json!({"type": "board_effect", "test": true, "control": control.id, "label": control.label,
        "effect": control.effect, "user": {"username": user.username}, "stream_ms": null,
        "at": Utc::now().timestamp_millis(), "overlay": row.overlay});
    app.chat
        .publish(&format!("overlay:{}", user.id), None, 0, effect.clone());
    Ok(Json(json!({"effect": effect})))
}

#[derive(Deserialize)]
pub struct Settings {
    moderators_run: bool,
    webhook_url: Option<String>,
}
/// PUT /api/me/board/settings: a new or changed webhook gets a new signing secret, shown once.
async fn settings(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<Settings>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let url = input
        .webhook_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(url) = url {
        webhook_target(&app, url).map_err(|m| Fail::field("webhook_url", m))?;
    }
    let current: Option<Option<String>> =
        sqlx::query_scalar("SELECT webhook_url FROM boards WHERE channel_id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    let fresh = url.is_some() && current.flatten().as_deref() != url;
    let secret = fresh.then(|| format!("whsec_{}", sec::token()));
    let sealed = secret
        .as_deref()
        .map(|s| sec::seal(&app, "board-webhook", s))
        .transpose()?;
    sqlx::query("INSERT INTO boards(channel_id,draft,moderators_run,webhook_url,webhook_secret) VALUES($1,$2,$3,$4,$5)
        ON CONFLICT(channel_id) DO UPDATE SET moderators_run=EXCLUDED.moderators_run, webhook_url=EXCLUDED.webhook_url,
            webhook_secret=CASE WHEN EXCLUDED.webhook_url IS NULL THEN NULL ELSE coalesce(EXCLUDED.webhook_secret,boards.webhook_secret) END, updated_at=now()")
        .bind(&user.id)
        .bind(sqlx::types::Json(Board::empty()))
        .bind(input.moderators_run)
        .bind(url)
        .bind(sealed)
        .execute(&app.db)
        .await?;
    let mut body = studio(&app, &user).await?;
    body["webhook_secret"] = json!(secret);
    Ok(Json(body))
}

/// POST /api/me/board/overlay: a new private overlay URL (the old one stops working).
async fn overlay_token(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_unrestricted(&mut *app.db.acquire().await?, &user.id).await?;
    let token = sec::token();
    sqlx::query("INSERT INTO boards(channel_id,draft,overlay_token_hash) VALUES($1,$2,$3) ON CONFLICT(channel_id) DO UPDATE SET overlay_token_hash=EXCLUDED.overlay_token_hash, overlay_seen_at=NULL")
        .bind(&user.id)
        .bind(sqlx::types::Json(Board::empty()))
        .bind(sec::digest(&token))
        .execute(&app.db)
        .await?;
    Ok(Json(
        json!({"url": format!("{}/overlay/{token}", app.config.origin)}),
    ))
}
/// DELETE /api/me/board/overlay
async fn overlay_revoke(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query(
        "UPDATE boards SET overlay_token_hash=NULL, overlay_seen_at=NULL WHERE channel_id=$1",
    )
    .bind(&user.id)
    .execute(&app.db)
    .await?;
    studio(&app, &user).await.map(Json)
}

// ---- Running the board (owner, staff, and moderators when allowed) ----

async fn runner(app: &App, jar: &CookieJar, channel: &str) -> Res<(auth::User, Role)> {
    let (user, role) = moderation::actor(app, jar, channel).await?;
    if role == Role::Moderator {
        let allowed: bool = sqlx::query_scalar(
            "SELECT coalesce((SELECT moderators_run FROM boards WHERE channel_id=$1),false)",
        )
        .bind(channel)
        .fetch_one(&app.db)
        .await?;
        if !allowed {
            return Err(Fail::denied(
                "The streamer hasn't let moderators run the board.",
            ));
        }
    }
    Ok((user, role))
}

#[derive(Deserialize)]
pub struct Panic {
    disabled: bool,
}
/// PUT /api/channels/{username}/board/disabled: the panic switch stops presses and effects at once.
async fn panic(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Panic>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (actor, role) = runner(&app, &jar, &channel).await?;
    let mut tx = app.db.begin().await?;
    let updated = sqlx::query("UPDATE boards SET disabled=$2 WHERE channel_id=$1")
        .bind(&channel)
        .bind(input.disabled)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if updated == 0 {
        return Err(Fail::missing());
    }
    let action = if input.disabled {
        "board_disable"
    } else {
        "board_enable"
    };
    moderation::log(
        &mut tx,
        &channel,
        &actor.id,
        role,
        action,
        None,
        None,
        json!({}),
        if input.disabled {
            "Board paused"
        } else {
            "Board resumed"
        },
    )
    .await?;
    tx.commit().await?;
    changed(&app, &channel);
    Ok(Json(json!({"disabled": input.disabled})))
}

/// PUT|DELETE /api/channels/{username}/board/blocks/{user}: block a viewer from pressing.
async fn block(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, target)): Path<(String, String)>,
) -> Res<Json<Value>> {
    set_block(app, jar, name, target, true).await
}
async fn unblock(
    State(app): State<App>,
    jar: CookieJar,
    Path((name, target)): Path<(String, String)>,
) -> Res<Json<Value>> {
    set_block(app, jar, name, target, false).await
}
async fn set_block(
    app: App,
    jar: CookieJar,
    name: String,
    target: String,
    on: bool,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (actor, role) = runner(&app, &jar, &channel).await?;
    let target: String = sqlx::query_scalar(
        "SELECT id FROM channel_users WHERE lower(username)=lower($1) AND deleted_at IS NULL",
    )
    .bind(&target)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(Fail::missing)?;
    if on && moderation::protected(&app, &channel, &actor.id, &target).await? {
        return Err(Fail::denied("You can't block this person from the board."));
    }
    let mut tx = app.db.begin().await?;
    let sql = if on {
        "INSERT INTO board_blocks(channel_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING"
    } else {
        "DELETE FROM board_blocks WHERE channel_id=$1 AND user_id=$2"
    };
    sqlx::query(sql)
        .bind(&channel)
        .bind(&target)
        .execute(&mut *tx)
        .await?;
    let action = if on { "board_block" } else { "board_unblock" };
    moderation::log(
        &mut tx,
        &channel,
        &actor.id,
        role,
        action,
        Some(&target),
        None,
        json!({}),
        if on {
            "Blocked from the board"
        } else {
            "Unblocked from the board"
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"blocked": on})))
}

// ---- Viewers ----

/// GET /api/channels/{username}/board: the published board, goal progress, the viewer's balance,
/// last presses (for cooldowns) and this stream's use (for limits).
async fn view(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let Some(row) = row(&app, &channel).await? else {
        return Ok(Json(json!({"board": null})));
    };
    let Some(board) = row.published else {
        return Ok(Json(json!({"board": null})));
    };
    let viewer = profiles::viewer(&app, &jar).await?;
    let live = live(&app, &channel).await?;
    let goals: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_object_agg(control_id,progress),'{}') FROM board_goals WHERE channel_id=$1",
    )
    .bind(&channel)
    .fetch_one(&app.db)
    .await?;
    let used: Value = sqlx::query_scalar("SELECT coalesce(jsonb_object_agg(control_id,n),'{}') FROM (SELECT control_id,count(*) AS n FROM board_presses WHERE channel_id=$1 AND broadcast_id=$2 GROUP BY 1) u")
        .bind(&channel)
        .bind(live.as_ref().map(|l| &l.0))
        .fetch_one(&app.db)
        .await?;
    let (mut balance, mut last, mut can_run, mut blocks) = (None, json!({}), false, Value::Null);
    if let Some(v) = &viewer {
        if v.id != channel {
            balance = Some(sqlx::query_scalar::<_, i64>("SELECT coalesce((SELECT balance FROM engagement WHERE channel_id=$1 AND user_id=$2),0)")
                .bind(&channel).bind(&v.id).fetch_one(&app.db).await?);
            last = sqlx::query_scalar("SELECT coalesce(jsonb_object_agg(control_id,at),'{}') FROM (SELECT control_id,max(created_at) AS at FROM board_presses WHERE channel_id=$1 AND user_id=$2 GROUP BY 1) p")
                .bind(&channel).bind(&v.id).fetch_one(&app.db).await?;
        }
        if let Some(role) = moderation::role_of(&app, &channel, v).await? {
            can_run = role != Role::Moderator || row.moderators_run;
        }
        if can_run {
            blocks = sqlx::query_scalar("SELECT coalesce(jsonb_agg(u.username ORDER BY u.username),'[]') FROM board_blocks b JOIN channel_users u ON u.id=b.user_id WHERE b.channel_id=$1")
                .bind(&channel).fetch_one(&app.db).await?;
        }
    }
    Ok(Json(json!({
        "board": board.0, "version": row.version, "disabled": row.disabled, "live": live.is_some(),
        "overlay": row.overlay, "goals": goals, "used": used, "last_press": last,
        "balance": balance, "signed_in": viewer.is_some(), "can_run": can_run, "blocks": blocks,
    })))
}

#[derive(Deserialize)]
pub struct Press {
    /// A UUID from the client, so a retried press is charged once.
    id: String,
    version: i32,
    control: String,
    text: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
}
/// POST /api/channels/{username}/board/press
async fn press(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Press>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::ensure_verified(&user, "Verify your email address to use the board.")?;
    let channel = channel(&app, &name).await?;
    if channel == user.id {
        return Err(Fail::bad(
            "Use test mode in Creator Studio to try your own board.",
        ));
    }
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid request ID."));
    }
    let row = row(&app, &channel).await?.ok_or_else(Fail::missing)?;
    let board = row.published.ok_or_else(Fail::missing)?.0;
    if row.disabled {
        return Err(Fail::conflict("The board is paused right now."));
    }
    if input.version != row.version {
        return Err(Fail::stale());
    }
    let control = board
        .control(&input.control)
        .filter(|c| pressable(c))
        .ok_or_else(Fail::missing)?;
    // Only real viewers: a counted or trusted playback session on this live broadcast.
    let (broadcast, stream_ms) = live(&app, &channel)
        .await?
        .ok_or_else(|| Fail::conflict("The board works while the stream is live."))?;
    let network: Option<Option<String>> = sqlx::query_scalar("SELECT net_hash FROM playback_leases WHERE broadcast_id=$1 AND viewer_key='u:'||$2 AND expires_at>now() AND level IN ('counted','trusted')")
        .bind(&broadcast)
        .bind(&user.id)
        .fetch_optional(&app.db)
        .await?;
    let Some(network) = network else {
        return Err(Fail::denied("Watch the stream to use the board."));
    };
    // Per account and per network, across all controls; joystick moves have their own budget
    // (at most 10 a second per viewer).
    let net = network.unwrap_or_else(|| format!("user:{}", user.id));
    let (prefix, window) = if control.kind == "joystick" {
        ("board-joy", 1)
    } else {
        ("board", 10)
    };
    sec::reserve(&app, vec![format!("{prefix}:{}", user.id)], 10, window).await?;
    sec::reserve(&app, vec![format!("{prefix}-net:{net}")], 50, window).await?;
    moderation::check_restriction(&app, &channel, &user.id).await?;
    let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM board_blocks WHERE channel_id=$1 AND user_id=$2) OR EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
        .bind(&channel)
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    if blocked {
        return Err(Fail::denied("You can't use this board."));
    }
    audience(&app, &channel, &user, &control.audience).await?;
    let author = json!({"username": user.username});
    let at = Utc::now().timestamp_millis();

    if control.kind == "joystick" {
        // ponytail: joystick moves are relayed, not stored (free, no outbox); persist them only
        // if a game or webhook ever needs replay.
        let (x, y) = (input.x.unwrap_or(0.0), input.y.unwrap_or(0.0));
        if !(-1.0..=1.0).contains(&x) || !(-1.0..=1.0).contains(&y) {
            return Err(Fail::bad("Joystick input is -1 to 1."));
        }
        app.chat.publish(&channel, Some(&user.id), 0, json!({"type": "board_input", "control": control.id,
            "x": x, "y": y, "user": author, "stream_ms": stream_ms, "at": at, "overlay": row.overlay}));
        return Ok(Json(json!({})));
    }
    let text = if control.kind == "text" {
        let text = input.text.as_deref().map(str::trim).unwrap_or_default();
        if !(1..=MAX_TEXT).contains(&text.chars().count()) {
            return Err(Fail::field("text", "Enter 1–200 characters."));
        }
        text::filter(text, "text")?;
        moderation::check_words(&app, &channel, &user, text).await?;
        Some(text.to_string())
    } else {
        None
    };

    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('board:'||$1||':'||$2, 7))")
        .bind(&channel)
        .bind(&control.id)
        .execute(&mut *tx)
        .await?;
    let repeat: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM board_presses WHERE id=$1 AND user_id=$2)")
            .bind(&input.id)
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
    if repeat {
        return Ok(Json(json!({"repeat": true})));
    }
    if control.cooldown_seconds > 0 {
        let ready: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT max(created_at)+make_interval(secs=>$4) FROM board_presses WHERE channel_id=$1 AND user_id=$2 AND control_id=$3")
            .bind(&channel).bind(&user.id).bind(&control.id).bind(f64::from(control.cooldown_seconds))
            .fetch_one(&mut *tx).await?;
        if let Some(ready) = ready.filter(|r| *r > Utc::now()) {
            return Err(Fail {
                retry: Some((ready - Utc::now()).num_seconds().max(1)),
                ..Fail::conflict("That control is cooling down.")
            });
        }
    }
    if let Some(limit) = control.per_stream_limit {
        let used: i64 = sqlx::query_scalar("SELECT count(*) FROM board_presses WHERE channel_id=$1 AND control_id=$2 AND broadcast_id=$3")
            .bind(&channel).bind(&control.id).bind(&broadcast)
            .fetch_one(&mut *tx).await?;
        if used >= i64::from(limit) {
            return Err(Fail::conflict(
                "That control has reached its limit for this stream.",
            ));
        }
    }
    let mut goal = None;
    if let Some(target) = control.target {
        let progress: i32 = sqlx::query_scalar("SELECT coalesce((SELECT progress FROM board_goals WHERE channel_id=$1 AND control_id=$2),0)")
            .bind(&channel).bind(&control.id).fetch_one(&mut *tx).await?;
        if progress >= target {
            return Err(Fail::conflict("That goal is complete."));
        }
        let progress = (progress + control.cost.max(1)).min(target);
        sqlx::query("INSERT INTO board_goals(channel_id,control_id,progress) VALUES($1,$2,$3) ON CONFLICT(channel_id,control_id) DO UPDATE SET progress=EXCLUDED.progress")
            .bind(&channel).bind(&control.id).bind(progress)
            .execute(&mut *tx).await?;
        goal = Some(json!({"progress": progress, "target": target, "reached": progress >= target}));
    }
    if control.cost > 0 {
        let paid = sqlx::query("UPDATE engagement SET balance=balance-$3 WHERE channel_id=$1 AND user_id=$2 AND balance>=$3")
            .bind(&channel).bind(&user.id).bind(i64::from(control.cost))
            .execute(&mut *tx).await?.rows_affected();
        if paid == 0 {
            return Err(Fail::conflict("You don't have enough Engagement Valor."));
        }
    }
    sqlx::query("INSERT INTO board_presses(id,channel_id,user_id,broadcast_id,version,control_id,cost,input) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&input.id).bind(&channel).bind(&user.id).bind(&broadcast).bind(row.version)
        .bind(&control.id).bind(control.cost).bind(text.as_ref().map(|t| json!({"text": t})))
        .execute(&mut *tx).await?;
    if row.webhook_url.is_some() {
        sqlx::query("INSERT INTO outbox(channel_id,kind,payload) VALUES($1,'webhook',$2)")
            .bind(&channel)
            .bind(json!({"type": "board.press", "id": input.id, "channel": name.to_lowercase(), "control": control.id,
                "label": control.label, "user": author, "text": text, "cost": control.cost, "goal": goal,
                "stream_ms": stream_ms, "created_at": Utc::now()}))
            .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    // The effect plays once: in the video when the overlay is connected, otherwise over the player.
    app.chat.publish(&channel, Some(&user.id), 0, json!({"type": "board_effect", "control": control.id,
        "label": control.label, "effect": control.effect, "user": author, "text": text, "goal": goal,
        "stream_ms": stream_ms, "at": at, "overlay": row.overlay}));
    let balance: i64 = sqlx::query_scalar(
        "SELECT coalesce((SELECT balance FROM engagement WHERE channel_id=$1 AND user_id=$2),0)",
    )
    .bind(&channel)
    .bind(&user.id)
    .fetch_one(&app.db)
    .await?;
    Ok(Json(json!({"balance": balance, "goal": goal})))
}
async fn audience(app: &App, channel: &str, user: &auth::User, audience: &str) -> Res<()> {
    let (ok, message) = match audience {
        "followers" => (
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2)",
            )
            .bind(&user.id)
            .bind(channel)
            .fetch_one(&app.db)
            .await?,
            "Follow the channel to use this control.",
        ),
        "subscribers" => (
            crate::subs::active_tier(app, channel, &user.id)
                .await?
                .is_some(),
            "This control is for subscribers.",
        ),
        "moderators" => (
            moderation::role_of(app, channel, user).await?.is_some(),
            "This control is for moderators.",
        ),
        _ => (true, ""),
    };
    if ok {
        Ok(())
    } else {
        Err(Fail::denied(message))
    }
}

// ---- OBS overlay ----

#[derive(Deserialize)]
pub struct OverlayQuery {
    token: String,
}
/// GET /api/boards/overlay/ws?token=: the browser source's socket. The private token is the only
/// credential (OBS has no session); it reaches effects and nothing else.
async fn overlay_socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<OverlayQuery>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("overlay-connect:{ip}")], 30, 60).await?;
    let digest = sec::digest(&query.token);
    let channel: String =
        sqlx::query_scalar("SELECT channel_id FROM boards WHERE overlay_token_hash=$1")
            .bind(&digest)
            .fetch_optional(&app.db)
            .await?
            .ok_or_else(Fail::missing)?;
    Ok(upgrade
        .max_message_size(1024)
        .on_upgrade(move |ws| overlay_session(app, channel, digest, ws)))
}
async fn overlay_session(app: App, channel: String, digest: String, mut ws: WebSocket) {
    let mut events = app.chat.subscribe();
    let test_room = format!("overlay:{channel}");
    let mut beat = tokio::time::interval(Duration::from_secs(10));
    beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = beat.tick() => {
                // Check in (so pages stop drawing effects) and stop if the token was replaced.
                let still = sqlx::query("UPDATE boards SET overlay_seen_at=now() WHERE channel_id=$1 AND overlay_token_hash=$2")
                    .bind(&channel).bind(&digest).execute(&app.db).await;
                if !matches!(still, Ok(r) if r.rows_affected() == 1) { return; }
            }
            event = events.recv() => match event {
                Ok(e) if forward(&e, &channel, &test_room) => {
                    if ws.send(Message::Text(e.payload.to_string().into())).await.is_err() { return; }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
fn forward(e: &Arc<Event>, channel: &str, test_room: &str) -> bool {
    (e.channel == channel || e.channel == test_room)
        && matches!(
            e.payload["type"].as_str(),
            Some("board_effect" | "board_input" | "board")
        )
}

// ---- Webhooks (outbox) ----

/// A webhook URL the platform will call: HTTPS (plain HTTP only in development), no credentials,
/// and never a private address (checked again against DNS at every delivery).
fn webhook_target(app: &App, url: &str) -> Result<url::Url, &'static str> {
    if url.len() > 500 {
        return Err("The URL is too long.");
    }
    let parsed = url::Url::parse(url).map_err(|_| "Enter a full https:// URL.")?;
    if !(parsed.scheme() == "https" || (!app.config.production && parsed.scheme() == "http")) {
        return Err("Webhooks must use https://.");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Don't put credentials in the URL.");
    }
    let private = match parsed.host() {
        None => return Err("Enter a full https:// URL."),
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => !public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => !public(IpAddr::V6(ip)),
    };
    if private && app.config.production {
        return Err("Webhooks can't go to private addresses.");
    }
    Ok(parsed)
}
/// Globally routable unicast only: no loopback, private, link-local, shared (CGNAT), benchmark,
/// documentation, multicast, reserved or NAT64 ranges.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_documentation()
                || v.is_multicast()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x64 && s[1] == 0xff9b)
                || s[0] == 0)
        }
    }
}
async fn deliver(app: &App, url: &str, secret: &str, body: &[u8]) -> Result<(), String> {
    let parsed = webhook_target(app, url).map_err(String::from)?;
    let host = parsed.host_str().ok_or("No host")?.to_string();
    let port = parsed.port_or_known_default().ok_or("No port")?;
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
        .await
        .map_err(|_| "The webhook's host didn't resolve.".to_string())?
        .collect();
    // Every address must be public, and the request goes to the one checked (no re-resolution).
    let Some(addr) = addrs.first().copied() else {
        return Err("The webhook's host didn't resolve.".into());
    };
    if app.config.production && addrs.iter().any(|a| !public(a.ip())) {
        return Err("The webhook's host points to a private address.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, addr)
        .user_agent("SVER-Webhooks/1")
        .build()
        .map_err(|_| "Internal error".to_string())?;
    let response = client
        .post(parsed)
        .header("content-type", "application/json")
        .header(
            "sver-signature",
            stripe::sign(secret, body, Utc::now().timestamp()),
        )
        .body(body.to_vec())
        .send()
        .await
        .map_err(|_| "The webhook didn't respond.".to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "The webhook answered HTTP {}.",
            response.status().as_u16()
        ))
    }
}

/// Sends due webhook deliveries; called every few seconds from its own loop. A failure retries
/// with doubling backoff, up to MAX_ATTEMPTS.
pub async fn deliver_due(app: &App) -> Res<()> {
    let due: Vec<(i64, String, Value, i32)> = sqlx::query_as("SELECT id,channel_id,payload,attempts FROM outbox WHERE delivered_at IS NULL AND available_at<=now() ORDER BY id LIMIT 20")
        .fetch_all(&app.db)
        .await?;
    for (id, channel, payload, attempts) in due {
        let target: Option<(Option<String>, Option<String>)> =
            sqlx::query_as("SELECT webhook_url,webhook_secret FROM boards WHERE channel_id=$1")
                .bind(&channel)
                .fetch_optional(&app.db)
                .await?;
        let Some((Some(url), Some(sealed))) = target else {
            // The streamer removed the webhook; nothing to send.
            sqlx::query("DELETE FROM outbox WHERE id=$1")
                .bind(id)
                .execute(&app.db)
                .await?;
            continue;
        };
        let secret = sec::unseal(app, "board-webhook", &sealed)?;
        let body = serde_json::to_vec(&payload).map_err(|_| Fail::internal())?;
        match deliver(app, &url, &secret, &body).await {
            Ok(()) => {
                sqlx::query("UPDATE outbox SET delivered_at=now(), attempts=attempts+1, error=NULL WHERE id=$1")
                    .bind(id).execute(&app.db).await?;
            }
            Err(error) => {
                sqlx::query("UPDATE outbox SET attempts=$2, error=$3, available_at=CASE WHEN $2>=$4 THEN 'infinity' ELSE now()+make_interval(secs=>10*power(2,$2-1)) END WHERE id=$1")
                    .bind(id).bind(attempts + 1).bind(error).bind(MAX_ATTEMPTS)
                    .execute(&app.db).await?;
            }
        }
    }
    sqlx::query("DELETE FROM outbox WHERE created_at<now()-interval '7 days' AND (delivered_at IS NOT NULL OR available_at='infinity')")
        .execute(&app.db)
        .await?;
    Ok(())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/board", get(mine))
        .route(
            "/api/me/board/draft",
            put(save).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/me/board/publish", post(publish))
        .route("/api/me/board/test", post(test))
        .route("/api/me/board/settings", put(settings))
        .route(
            "/api/me/board/overlay",
            post(overlay_token).delete(overlay_revoke),
        )
        .route("/api/channels/{username}/board", get(view))
        .route("/api/channels/{username}/board/press", post(press))
        .route("/api/channels/{username}/board/disabled", put(panic))
        .route(
            "/api/channels/{username}/board/blocks/{user}",
            put(block).delete(unblock),
        )
        .route("/api/boards/overlay/ws", get(overlay_socket))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_receive_webhooks() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "2001:db8::1",
        ] {
            assert!(!public(private.parse().unwrap()), "{private}");
        }
        for open in ["93.184.216.34", "1.1.1.1", "2606:4700::1111"] {
            assert!(public(open.parse().unwrap()), "{open}");
        }
    }

    #[test]
    fn templates_are_valid_and_rules_hold() {
        for (name, board) in templates() {
            validate(&board).unwrap_or_else(|e| panic!("{name}: {}", e.message));
        }
        let mut board = templates()[0].1.clone();
        board.screens[0].controls[1].id = board.screens[0].controls[0].id.clone();
        assert!(validate(&board).is_err(), "duplicate IDs");
        let mut board = templates()[3].1.clone();
        board.screens[0].controls[0].cost = 5;
        assert!(validate(&board).is_err(), "joysticks are free");
        let mut board = templates()[1].1.clone();
        board.screens[0].controls[1].target = None;
        assert!(validate(&board).is_err(), "goals need a target");
        assert!(validate(&Board { screens: vec![] }).is_err());
    }
}
