//! Module 3 chat: persisted channel messages, one in-process fanout hub, HTTP and WebSocket.
//! HTTP sends and socket commands share `send`, so validation and permissions are identical.
use crate::{
    App, moderation,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Json, Router,
    extract::{
        ConnectInfo, Path, Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, put},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

const HISTORY: i64 = 100;
const FRAME: usize = 8 * 1024;
const MAX_SOCKETS: usize = 10_000;
/// Open chat sockets one address may hold, so no single network can fill `MAX_SOCKETS`. Generous
/// because a campus or carrier shares one address among many viewers.
const MAX_PER_ADDRESS: usize = 100;
/// A socket that hasn't answered a ping (browsers do so on their own) for this long is closed.
const SILENT: Duration = Duration::from_secs(75);
/// Events a room holds for a reader that hasn't caught up; one further behind resyncs. Only the
/// room's own traffic counts.
const ROOM: usize = 512;

/// Fanout for a single API instance. Running replicas needs a tested cross-instance design first.
#[derive(Clone)]
pub struct Hub {
    /// Every event on this instance, for the app events socket alone (its rooms come and go).
    firehose: broadcast::Sender<Arc<Event>>,
    /// One sender per room someone listens to, so a socket only wakes for its own rooms.
    rooms: Arc<Mutex<HashMap<String, broadcast::Sender<Arc<Event>>>>>,
    /// Each MAGNet room (`magnet:{lane}`) merged with a chat now: that chat's room and owners.
    /// The merged room's lines and the owners' emote changes are passed on to the MAGNet room.
    lanes: Arc<Mutex<HashMap<String, LaneMerge>>>,
    /// Each MAGNet room as everyone sees it, read once for all its sockets (`hype_snapshot`).
    hype: Arc<Shared<Arc<HypeRoom>>>,
    sockets: Arc<AtomicUsize>,
    addresses: Arc<Mutex<HashMap<IpAddr, usize>>>,
    /// Each channel's chat rules (emotes and banner state), shared by every socket for five seconds.
    rules: Arc<Shared<Rules>>,
    /// Database reads behind `rules`, so a test can see a burst of sockets share one.
    rule_reads: Arc<AtomicUsize>,
    /// Each shared chat's open roster (None once it ended), shared by its sockets for a second.
    squads: Arc<Shared<Option<Vec<String>>>>,
    /// The merged co-stream each channel's own chat has moved into, shared by its sockets for five
    /// seconds, so open chats learn to switch rooms without every viewer asking.
    merged: Arc<Shared<Option<String>>>,
    /// Bumped after any block or unblock commits, so sockets re-read their block list only then.
    blocks: Arc<AtomicUsize>,
    /// Bumped after a ban or a shared-chat restriction commits, so shared-chat sockets re-check
    /// the viewer's access only then (and on a slow safety tick).
    access: Arc<AtomicUsize>,
    /// Each channel's latest pin, read once per change for every socket (`pin_view` filters it).
    pins: Arc<Mutex<HashMap<String, Pinned>>>,
    /// Pin reads run one at a time, so the newest read is always the one stored.
    // ponytail: one lock site-wide; pins are rare moderator actions, per-channel locks if not.
    pin_reads: Arc<tokio::sync::Mutex<()>>,
    /// Messages deleted in the last minute, so a message event published just after its own
    /// delete (a removal racing the send's fanout) is never delivered. Kept with the last prune,
    /// so a burst of deletes doesn't rescan the map on every one.
    deleted: Arc<Mutex<(Instant, HashMap<String, Instant>)>>,
}
/// Where a MAGNet room's chat merges: the merged room and its owners; None while it doesn't.
type LaneMerge = Option<(String, Vec<String>)>;
/// A pinned message as everyone sees it (badges hydrated, quote author kept) and its author.
type Pinned = Option<(String, Value)>;
/// Values shared by every socket that asks within an age, each read at most once at a time
/// however many ask together.
struct Shared<T>(Mutex<HashMap<String, Slot<T>>>);
/// One key's value and when it was read; held while it is being read.
type Slot<T> = Arc<tokio::sync::Mutex<Option<(Instant, T)>>>;
impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}
impl<T: Clone> Shared<T> {
    /// With `after`, only a read that began after it will do (one that sees what was committed
    /// by then).
    async fn get<F: std::future::Future<Output = Res<T>>>(
        &self,
        key: &str,
        age: Duration,
        after: Option<Instant>,
        read: impl FnOnce() -> F,
    ) -> Res<T> {
        let fresh =
            |slot: &Option<(Instant, T)>| slot.as_ref().is_some_and(|(at, _)| at.elapsed() < age);
        let slot = {
            let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if !all.contains_key(key) {
                all.retain(|_, slot| slot.try_lock().map_or(true, |s| fresh(&s)));
            }
            all.entry(key.into()).or_default().clone()
        };
        let mut slot = slot.lock().await;
        if fresh(&slot)
            && let Some((at, value)) = slot.as_ref()
            && after.is_none_or(|after| *at >= after)
        {
            return Ok(value.clone());
        }
        let began = Instant::now();
        let value = read().await?;
        *slot = Some((began, value.clone()));
        Ok(value)
    }
    /// The next reader reads again (a read already under way finishes for its own callers only).
    fn forget(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}
/// What a chat shows besides its messages: its emotes and what its banner says.
#[derive(Clone, Default, PartialEq)]
struct Rules {
    emotes: Vec<Value>,
    allow_signatures: bool,
    subs_only: bool,
    followers_only: Option<DateTime<Utc>>,
}
impl Default for Hub {
    fn default() -> Self {
        Self {
            firehose: broadcast::channel(4096).0,
            rooms: Arc::default(),
            lanes: Arc::default(),
            hype: Arc::default(),
            sockets: Arc::default(),
            addresses: Arc::default(),
            rules: Arc::default(),
            rule_reads: Arc::default(),
            squads: Arc::default(),
            merged: Arc::default(),
            blocks: Arc::default(),
            access: Arc::default(),
            pins: Arc::default(),
            pin_reads: Arc::default(),
            deleted: Arc::new(Mutex::new((Instant::now(), HashMap::new()))),
        }
    }
}
/// The address limits count by: an IPv6 /64 is one subscriber, so it counts as one address.
pub(crate) fn network(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & (!0u128 << 64))),
        v4 => v4,
    }
}
pub struct Event {
    pub(crate) channel: String,
    pub(crate) author: Option<String>,
    seq: i64,
    pub(crate) payload: Value,
    /// When it was published.
    at: Instant,
}
/// A socket's rooms read as one: a receiver per room, so another room's traffic never wakes it
/// or makes it fall behind.
#[derive(Default)]
pub(crate) struct Rooms(Vec<(String, broadcast::Receiver<Arc<Event>>)>);
impl Rooms {
    /// Listens to exactly these rooms, keeping the receivers it already holds.
    pub(crate) fn set(&mut self, hub: &Hub, rooms: &[String]) {
        self.0.retain(|(room, _)| rooms.contains(room));
        for room in rooms {
            if !self.0.iter().any(|(r, _)| r == room) {
                self.0.push((room.clone(), hub.subscribe_room(room)));
            }
        }
    }
    /// The next event from any of its rooms (none ever, while it has no rooms).
    pub(crate) async fn recv(&mut self) -> Result<Arc<Event>, broadcast::error::RecvError> {
        if self.0.is_empty() {
            return std::future::pending().await;
        }
        futures_util::future::select_all(self.0.iter_mut().map(|(_, rx)| Box::pin(rx.recv())))
            .await
            .0
    }
}
impl Hub {
    /// Every event on this instance: only the app events socket, whose chat topics come and go.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.firehose.subscribe()
    }
    /// One room's events. Subscribe before reading what the room already holds.
    pub(crate) fn subscribe_room(&self, room: &str) -> broadcast::Receiver<Arc<Event>> {
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = rooms.get(room) {
            return tx.subscribe();
        }
        // A quiet room may never be published to again, so empty ones are dropped here too.
        rooms.retain(|_, tx| tx.receiver_count() > 0);
        // The app events room takes a whole outbox drain (up to 1000 rows) in one pass.
        let (tx, rx) = broadcast::channel(if room == crate::events::HUB {
            4096
        } else {
            ROOM
        });
        rooms.insert(room.into(), tx);
        rx
    }
    pub fn publish(&self, channel: &str, author: Option<&str>, seq: i64, payload: Value) {
        if payload["type"] == "delete"
            && let Some(id) = payload["id"].as_str()
        {
            let mut guard = self.deleted.lock().unwrap_or_else(|e| e.into_inner());
            let (pruned, deleted) = &mut *guard;
            if pruned.elapsed() >= Duration::from_secs(1) {
                deleted.retain(|_, at| at.elapsed() < Duration::from_secs(60));
                *pruned = Instant::now();
            }
            deleted.insert(id.into(), Instant::now());
        }
        // The next socket to hear it reads the new rules; the rest share that read.
        if matches!(
            payload["type"].as_str(),
            Some("emotes" | "subs_only" | "protect")
        ) {
            self.rules.forget(channel);
        }
        // MAGNet rooms merged with this chat hear its lines, and its owners' emote changes.
        let kind = payload["type"].as_str().unwrap_or_default();
        let lanes: Vec<String> = if matches!(kind, "message" | "delete" | "emotes") {
            let lanes = self.lanes.lock().unwrap_or_else(|e| e.into_inner());
            lanes
                .iter()
                .filter(|(_, merge)| match (merge, kind) {
                    (Some((_, owners)), "emotes") => owners.iter().any(|o| o == channel),
                    (Some((room, _)), _) => room == channel,
                    (None, _) => false,
                })
                .map(|(lane, _)| lane.clone())
                .collect()
        } else {
            Vec::new()
        };
        // A MAGNet room that hears anything it shows is read again by the next reader (linked
        // platforms' and the bot's lines stay in the channel's own chat).
        let shown = kind != "message" || payload["message"]["outside"].is_null();
        for room in std::iter::once(channel)
            .filter(|c| c.starts_with("magnet:"))
            .chain(lanes.iter().map(String::as_str))
            .filter(|_| shown)
        {
            self.hype.forget(room);
        }
        let event = Arc::new(Event {
            channel: channel.into(),
            author: author.map(Into::into),
            seq,
            payload,
            at: Instant::now(),
        });
        let mut rooms = self.rooms.lock().unwrap_or_else(|e| e.into_inner());
        for room in std::iter::once(channel).chain(lanes.iter().map(String::as_str)) {
            // Sending fails only once every reader left.
            if rooms
                .get(room)
                .is_some_and(|tx| tx.send(event.clone()).is_err())
            {
                rooms.remove(room);
            }
        }
        drop(rooms);
        let _ = self.firehose.send(event);
    }
    /// Call after a block or unblock commits.
    pub(crate) fn blocks_changed(&self) {
        self.blocks.fetch_add(1, Ordering::Release);
    }
    /// Call after a channel ban or a shared-chat restriction commits.
    pub(crate) fn access_changed(&self) {
        self.access.fetch_add(1, Ordering::Release);
    }
    pub(crate) async fn merged(&self, app: &App, channel: &str) -> Res<Option<String>> {
        self.merged
            .get(channel, Duration::from_secs(5), None, || {
                merged_squad(app, channel)
            })
            .await
    }
    /// Call after a co-stream's roster changes, so its sockets see it at once.
    pub(crate) fn squad_changed(&self, id: &str) {
        self.squads.forget(id);
    }
    /// Moves whenever a shared-chat viewer's access could have changed.
    fn access_generation(&self) -> usize {
        self.access
            .load(Ordering::Acquire)
            .wrapping_add(self.blocks.load(Ordering::Acquire))
    }
    /// How many times `rules` has read the database.
    pub fn rule_reads(&self) -> usize {
        self.rule_reads.load(Ordering::Relaxed)
    }
    fn deleted(&self, id: &str) -> bool {
        self.deleted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .get(id)
            .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
    }
    /// A restored message may be delivered again.
    fn restored(&self, id: &str) {
        self.deleted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(id);
    }
    /// Takes a socket slot at connect (released when the socket closes), or refuses.
    pub(crate) fn open(&self, ip: IpAddr) -> Res<SocketSlot> {
        let mut open = self.addresses.lock().unwrap_or_else(|e| e.into_inner());
        if self.sockets.load(Ordering::Relaxed) >= MAX_SOCKETS {
            return Err(Fail::unavailable("Chat is busy. Please try again shortly."));
        }
        let n = open.entry(ip).or_default();
        if *n >= MAX_PER_ADDRESS {
            return Err(Fail::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "Too many chat connections from your network.",
            ));
        }
        *n += 1;
        self.sockets.fetch_add(1, Ordering::Relaxed);
        Ok(SocketSlot(self.clone(), ip))
    }
    /// The channel's rules, read at most once every five seconds however many sockets ask.
    async fn rules(&self, app: &App, channel: &str) -> Res<Rules> {
        self.rules
            .get(channel, Duration::from_secs(5), None, || async {
                self.rule_reads.fetch_add(1, Ordering::Relaxed);
                let (allow_signatures, subs_only, followers_only) = sqlx::query_as("SELECT coalesce(s.allow_signatures,true),coalesce(s.subs_only,false),CASE WHEN s.followers_only_until>now() THEN s.followers_only_until END FROM (SELECT 1) x LEFT JOIN chat_settings s ON s.channel_id=$1")
                    .bind(channel)
                    .fetch_one(&app.db)
                    .await?;
                Ok(Rules {
                    emotes: crate::emotes::catalog(app, channel).await?,
                    allow_signatures,
                    subs_only,
                    followers_only,
                })
            })
            .await
    }
    /// A shared chat's open roster, host first: read at most once a second however many ask.
    pub(crate) async fn squad(&self, app: &App, id: &str) -> Res<Option<Vec<String>>> {
        self.squads
            .get(id, Duration::from_secs(1), None, || async {
                crate::squads::open_members(&mut *app.db.acquire().await?, id).await
            })
            .await
    }
    /// The channel's pin as of now, stored for every socket to filter. Reads run one at a time
    /// after the change commits, so the last one stored is the newest.
    async fn repin(&self, app: &App, channel: &str) -> Res<Pinned> {
        let _turn = self.pin_reads.lock().await;
        let pin = pin_row(app, channel).await;
        let mut pins = self.pins.lock().unwrap_or_else(|e| e.into_inner());
        match &pin {
            Ok(pin) => pins.insert(channel.into(), pin.clone()),
            // Sockets then read it themselves.
            Err(_) => pins.remove(channel),
        };
        pin
    }
    fn pin(&self, channel: &str) -> Option<Pinned> {
        self.pins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel)
            .cloned()
    }
}
/// A ping every 30 seconds, the first one 30 seconds after connect.
fn pings() -> tokio::time::Interval {
    let every = Duration::from_secs(30);
    tokio::time::interval_at(tokio::time::Instant::now() + every, every)
}
pub(crate) struct SocketSlot(Hub, IpAddr);
/// One socket's view of who it hides (`hidden_for` over its chat's channels): re-read after a
/// block changes anywhere, after the scope changes, or on `recheck` when a role could matter.
pub(crate) struct Hidden {
    /// The block generation read with `ids`; `STALE` forces a read.
    seen: usize,
    scope: Vec<String>,
    /// Someone blocks this viewer, so whether they moderate the scope changes the set.
    role_matters: bool,
    ids: HashSet<String>,
}
const STALE: usize = usize::MAX;
impl Hidden {
    /// Read lazily, on the first `current`.
    pub(crate) fn new(scope: Vec<String>) -> Self {
        Self {
            seen: STALE,
            scope,
            role_matters: false,
            ids: HashSet::new(),
        }
    }
    /// The current set, re-read only if it went stale since the last read.
    pub(crate) async fn current(
        &mut self,
        app: &App,
        viewer: Option<&str>,
    ) -> Res<&HashSet<String>> {
        let generation = app.chat.blocks.load(Ordering::Acquire);
        if generation != self.seen {
            // The generation is read first: a block committed during the query bumps it again.
            (self.ids, self.role_matters) = hidden_for(app, viewer, &self.scope).await?;
            self.seen = generation;
        }
        Ok(&self.ids)
    }
    /// A different set of channels (a co-stream's members changed, MAGNet switched).
    pub(crate) fn rescope(&mut self, scope: Vec<String>) {
        if scope != self.scope {
            self.scope = scope;
            self.seen = STALE;
        }
    }
    /// On a refresh tick: appointing or removing a moderator takes effect without a reconnect.
    pub(crate) fn recheck(&mut self) {
        if self.role_matters {
            self.seen = STALE;
        }
    }
}
impl Drop for SocketSlot {
    fn drop(&mut self) {
        let mut open = self.0.addresses.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = open.get_mut(&self.1) {
            *n -= 1;
            if *n == 0 {
                open.remove(&self.1);
            }
        }
        self.0.sockets.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct Row {
    id: String,
    seq: i64,
    author_id: String,
    body: String,
    created_at: DateTime<Utc>,
    author: Value,
    role: Option<String>,
    mentions: Vec<String>,
    reply: Option<Value>,
    /// The MAGNet Hype lane a message came from; None for a channel's own chat.
    origin: Option<String>,
    /// Valor paid with this message (a tribute, docs/SUPPORT.md).
    tribute: Option<i32>,
    /// The author's active subscription to this channel, for the badge: tier and months. In
    /// shared chat also the channel's username: the host's when they sub there, else the member
    /// channel they've subbed to longest.
    sub: Option<Value>,
    /// Paid for with the channel's "highlight my message" reward.
    highlighted: bool,
    /// The author's creator tier (1-3), for their badge.
    creator_tier: Option<i16>,
    /// The Skill this paid message played.
    skill: Option<String>,
    /// Lifetime Engagement Valor the author earned in this channel (their loyalty rank).
    earned: Option<i64>,
    /// A KLIPY GIF (docs/COMMUNITY.md "GIFs in chat"); the body then holds its alt text.
    gif: Option<Value>,
    /// The author's platform supporter tier, months and Founder number, only while subscribed
    /// (docs/SUPPORTERS.md). Decided here, never by the client.
    supporter: Option<Value>,
    /// Live, unlike `role` (stored at send): the author is the owner, a moderator or staff now,
    /// so nobody times them out or bans them from this line (`moderation::protected`).
    protected: bool,
    /// Live: the author moderates this channel now (in shared chat: any member channel).
    moderator: bool,
    /// Read only where a query asks for it (a MAGNet room, whose lines come from many channels).
    #[sqlx(default)]
    channel_id: Option<String>,
}
impl Row {
    async fn hydrated(self, app: &App) -> Res<Value> {
        let mut value = self.json(app);
        crate::guilds::hydrate_badges(app, std::slice::from_mut(&mut value)).await?;
        Ok(value)
    }
    pub(crate) fn json(mut self, app: &App) -> Value {
        profiles::hydrate(app, &mut self.author);
        json!({"id": self.id, "seq": self.seq, "author": self.author, "body": self.body, "created_at": self.created_at, "role": self.role, "mentions":self.mentions, "reply":self.reply, "origin":self.origin, "tribute":self.tribute, "sub":self.sub, "highlighted":self.highlighted, "creator_tier":self.creator_tier, "skill":self.skill, "gif":self.gif, "supporter":self.supporter, "protected":self.protected, "moderator":self.moderator, "loyalty":crate::progression::loyalty(self.earned.unwrap_or(0))})
    }
}
pub(crate) fn select() -> String {
    format!(
        "SELECT m.id,m.seq,m.author_id,m.body,m.created_at,{} || jsonb_build_object('guild',{}) AS author,m.role,m.origin,m.tribute,m.highlighted,m.skill,m.gif,
        (SELECT t.tier FROM creator_tiers t WHERE t.user_id=m.author_id AND t.tier>0) AS creator_tier,
        (SELECT e.earned FROM engagement e WHERE e.channel_id=coalesce(m.credit_id,m.channel_id) AND e.user_id=m.author_id) AS earned,
        (SELECT jsonb_strip_nulls(jsonb_build_object('tier',s.tier,'months',s.months,'channel',CASE WHEN m.squad_id IS NOT NULL THEN su.username END)) FROM channel_subs s JOIN users su ON su.id=s.channel_id WHERE s.user_id=m.author_id AND s.paid_through>now()
            AND (s.channel_id=m.channel_id OR s.channel_id IN (SELECT q.user_id FROM squad_members q WHERE q.squad_id=m.squad_id))
            ORDER BY s.channel_id=m.channel_id DESC,s.months DESC,su.username LIMIT 1) AS sub,
        {} AS supporter,
        EXISTS(SELECT 1 FROM channel_moderators d WHERE d.user_id=m.author_id AND (d.channel_id=m.channel_id OR d.channel_id IN (SELECT q.user_id FROM squad_members q WHERE q.squad_id=m.squad_id))) AS moderator,
        (coalesce(m.author_id=m.channel_id,false) OR m.author_id IN (SELECT q.user_id FROM squad_members q WHERE q.squad_id=m.squad_id)
            OR EXISTS(SELECT 1 FROM channel_moderators d WHERE d.user_id=m.author_id AND (d.channel_id=m.channel_id OR d.channel_id IN (SELECT q.user_id FROM squad_members q WHERE q.squad_id=m.squad_id)))
            OR EXISTS(SELECT 1 FROM staff_roles sr WHERE sr.user_id=m.author_id)) AS protected,
        ARRAY(SELECT username FROM channel_users WHERE id=ANY(m.mention_ids) AND eligible) AS mentions,
        CASE WHEN m.reply_to IS NOT NULL THEN jsonb_build_object('id',m.reply_to,
            'author_id',r.author_id,'username',ra.username,
            'body',left(regexp_replace(r.body,E'[\\n\\r]+',' ','g'),80)) END AS reply
        FROM chat_messages m JOIN channel_users a ON a.id=m.author_id
        LEFT JOIN chat_messages r ON r.id=m.reply_to AND r.channel_id=m.channel_id
            AND r.squad_id IS NOT DISTINCT FROM m.squad_id
            AND r.deleted_at IS NULL AND (r.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=r.id))
        LEFT JOIN channel_users ra ON ra.id=r.author_id",
        profiles::chip_sql("a"), crate::guilds::badge_sql("a.id"), crate::supporters::active_sql("m.author_id")
    )
}

// Apply the same block rule to a quote as to its original message; never expose its internal ID.
pub(crate) fn visible_message(mut message: Value, hidden: &HashSet<String>) -> Value {
    if let Some(reply) = message["reply"].as_object_mut() {
        let blocked = reply
            .remove("author_id")
            .and_then(|v| v.as_str().map(|id| hidden.contains(id)))
            .unwrap_or(false);
        if blocked {
            reply.insert("body".into(), Value::Null);
            reply.insert("username".into(), Value::Null);
        }
    }
    message
}

pub(crate) const VISIBLE: &str = "m.deleted_at IS NULL AND (m.expires_at>now() OR EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id))";
/// Public read projection for recording replay. The chat module retains ownership of its
/// visibility rules; recordings supply their segment-to-wall-clock mapping around this query.
pub fn replay_source_sql() -> String {
    format!(
        "SELECT m.id,m.author_id,m.channel_id,m.squad_id,m.created_at,jsonb_build_object('id',m.id,'body',m.body,'author',{},'role',m.role,'created_at',m.created_at) AS message FROM chat_messages m JOIN channel_users a ON a.id=m.author_id WHERE {VISIBLE} AND a.eligible",
        profiles::chip_sql("a")
    )
}
pub async fn replay_redacted(db: &mut sqlx::PgConnection, ids: &[String]) -> Res<HashSet<String>> {
    Ok(sqlx::query_scalar::<_,String>("SELECT m.id FROM chat_messages m LEFT JOIN channel_users u ON u.id=m.author_id WHERE m.id=ANY($1) AND (m.deleted_at IS NOT NULL OR NOT coalesce(u.eligible,false))").bind(ids).fetch_all(db).await?.into_iter().collect())
}

/// The channel's pin for everyone, unfiltered: `pin_view` makes one viewer's copy.
async fn pin_row(app: &App, channel: &str) -> Res<Pinned> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} JOIN chat_pins p ON p.message_id=m.id AND p.channel_id=m.channel_id WHERE m.channel_id=$1 AND m.squad_id IS NULL AND {VISIBLE}", select()
    ))).bind(channel).fetch_optional(&app.db).await?;
    Ok(match row {
        Some(r) => Some((r.author_id.clone(), r.hydrated(app).await?)),
        None => None,
    })
}
/// A pin as one viewer sees it: none when they block its author or it was just deleted, and a
/// quote of someone they block is blanked.
fn pin_view(hub: &Hub, pin: &Pinned, hidden: &HashSet<String>) -> Value {
    match pin {
        Some((author, message))
            if !hidden.contains(author) && !hub.deleted(message["id"].as_str().unwrap_or("")) =>
        {
            visible_message(message.clone(), hidden)
        }
        _ => Value::Null,
    }
}
async fn pinned(app: &App, channel: &str, hidden: &HashSet<String>) -> Res<Value> {
    Ok(pin_view(&app.chat, &pin_row(app, channel).await?, hidden))
}

/// Tells open chats that a message is gone: its room, the Hype room it came from, and every Hype
/// room that merged with its channel while it was posted (they keep that window's lines after
/// MAGNet moves on). Those are looked up in the background.
pub(crate) fn publish_delete(app: &App, channel: Option<&str>, origin: Option<&str>, id: &str) {
    let delete = json!({"type":"delete","id":id});
    if let Some(channel) = channel {
        app.chat.publish(channel, None, 0, delete.clone());
    }
    if let Some(lane) = origin {
        app.chat
            .publish(&format!("magnet:{lane}"), None, 0, delete.clone());
    }
    if channel.is_none() {
        return;
    }
    let (app, id, origin) = (app.clone(), id.to_string(), origin.map(String::from));
    tokio::spawn(async move {
        let Ok(lanes) = sqlx::query_scalar::<_, String>("SELECT DISTINCT f.lane FROM chat_messages m JOIN magnet_features f ON f.owner_id=m.channel_id AND f.merged AND m.created_at>=f.started_at AND (f.ended_at IS NULL OR m.created_at<f.ended_at) WHERE m.id=$1")
            .bind(&id).fetch_all(&app.db).await
        else {
            eprintln!("chat_event=delete_lanes outcome=failed");
            return;
        };
        for lane in lanes
            .iter()
            .filter(|l| origin.as_deref() != Some(l.as_str()))
        {
            app.chat
                .publish(&format!("magnet:{lane}"), None, 0, delete.clone());
        }
    });
}

/// An active pin is retained until removed; ordinary chat still expires after seven days.
pub async fn expire(app: &App) -> crate::Result<()> {
    let mut tx = app.db.begin().await?;
    // Lock candidates, then recheck pins in a fresh statement snapshot. A pin committed while
    // cleanup was waiting on a message must not disappear with that message's ordinary expiry.
    let ids: Vec<String> = sqlx::query_scalar("SELECT m.id FROM chat_messages m WHERE expires_at<=now() AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) ORDER BY expires_at LIMIT 1000 FOR UPDATE SKIP LOCKED")
        .fetch_all(&mut *tx).await?;
    let redacted = replay_redacted(&mut tx, &ids)
        .await
        .map_err(|_| crate::Error::internal())?;
    crate::videos::redact_messages(&mut tx, &redacted.into_iter().collect::<Vec<_>>())
        .await
        .map_err(|_| crate::Error::internal())?;
    let removed: Vec<(Option<String>, Option<String>, String)> = sqlx::query_as("DELETE FROM chat_messages m WHERE id=ANY($1) AND NOT EXISTS(SELECT 1 FROM chat_pins WHERE message_id=m.id) RETURNING coalesce(squad_id,channel_id),origin,id")
        .bind(ids).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    // Gone outright, so there's no row left to find its merged MAGNet windows by: only its room
    // and origin hear.
    for (channel, origin, id) in removed {
        if let Some(channel) = channel {
            app.chat
                .publish(&channel, None, 0, json!({"type":"delete","id":id}));
        }
        publish_delete(app, None, origin.as_deref(), &id);
    }
    Ok(())
}

/// Whole ASCII usernames only: no email addresses, partial long names or HTML parsing.
fn mention_names(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut names = HashSet::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '@'
            || (i > 0 && (chars[i - 1].is_alphanumeric() || matches!(chars[i - 1], '_' | '@')))
        {
            continue;
        }
        let name: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .collect();
        if (3..=25).contains(&name.len())
            && !chars
                .get(i + 1 + name.len())
                .is_some_and(|c| c.is_alphanumeric() || *c == '_')
        {
            names.insert(name.to_ascii_lowercase());
        }
    }
    names.into_iter().collect()
}

/// Refresh a moderation change for already-connected viewers after its database commit.
pub async fn notify_changed(app: &App, id: &str) -> Res<()> {
    let row: Option<(Option<String>, Option<String>, bool)> =
        sqlx::query_as("SELECT coalesce(squad_id,channel_id),origin,deleted_at IS NOT NULL FROM chat_messages WHERE id=$1")
            .bind(id)
            .fetch_optional(&app.db)
            .await?;
    let Some((channel, origin, hidden)) = row else {
        return Ok(());
    };
    if hidden {
        publish_delete(app, channel.as_deref(), origin.as_deref(), id);
    } else {
        let message: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{} WHERE m.id=$1 AND {VISIBLE}",
            select()
        )))
        .bind(id)
        .fetch_optional(&app.db)
        .await?;
        if let Some(message) = message {
            app.chat.restored(id);
            let author = message.author_id.clone();
            let event = json!({"type":"message","message":message.hydrated(app).await?});
            if let Some(channel) = channel {
                app.chat.publish(&channel, Some(&author), 0, event.clone());
            }
            if let Some(lane) = origin {
                app.chat
                    .publish(&format!("magnet:{lane}"), Some(&author), 0, event);
            }
        }
    }
    Ok(())
}

/// The channel's owner id; unknown, deleted and restricted channels get the same 404 as profiles.
async fn channel(app: &App, name: &str) -> Res<String> {
    let mut conn = app.db.acquire().await?;
    Ok(profiles::eligible_by_name(&mut conn, name)
        .await?
        .ok_or_else(Fail::channel_missing)?
        .id)
}
/// People whose messages the viewer doesn't see: blocks in either direction.
pub(crate) async fn hidden(app: &App, viewer: Option<&str>) -> Res<HashSet<String>> {
    Ok(hidden_for(app, viewer, &[]).await?.0)
}
/// `hidden` for reading the chat of these channels. Blocking whoever moderates them doesn't
/// defeat moderation: their owner, moderators and staff hide only the people they block
/// themselves. Also returns whether anyone blocks the viewer (only then does the role matter).
pub(crate) async fn hidden_for(
    app: &App,
    viewer: Option<&str>,
    scope: &[String],
) -> Res<(HashSet<String>, bool)> {
    let Some(viewer) = viewer else {
        return Ok((HashSet::new(), false));
    };
    let rows: Vec<(String, bool, bool)> = sqlx::query_as("SELECT blocked_id,true,false FROM user_blocks WHERE blocker_id=$1
        UNION ALL SELECT blocker_id,false,cardinality($2::text[])>0 AND ($1=ANY($2) OR EXISTS(SELECT 1 FROM channel_moderators WHERE channel_id=ANY($2) AND user_id=$1) OR EXISTS(SELECT 1 FROM staff_roles WHERE user_id=$1))
        FROM user_blocks WHERE blocked_id=$1")
        .bind(viewer).bind(scope).fetch_all(&app.db).await?;
    let blocked_by = rows.iter().any(|(_, mine, _)| !mine);
    Ok((
        rows.into_iter()
            .filter(|(_, mine, moderates)| *mine || !moderates)
            .map(|(id, ..)| id)
            .collect(),
        blocked_by,
    ))
}
pub(crate) async fn history(
    app: &App,
    channel: &str,
    hidden: &HashSet<String>,
    squad: Option<&str>,
) -> Res<Vec<Value>> {
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE m.channel_id=$1 AND m.squad_id IS NOT DISTINCT FROM $3 AND {VISIBLE} ORDER BY m.seq DESC LIMIT $2",
        select()
    )))
    .bind(channel)
    .bind(HISTORY)
    .bind(squad)
    .fetch_all(&app.db)
    .await?;
    let mut messages: Vec<Value> = rows
        .into_iter()
        .rev()
        .filter(|r| !hidden.contains(&r.author_id))
        .map(|r| visible_message(r.json(app), hidden))
        .collect();
    crate::guilds::hydrate_badges(app, &mut messages).await?;
    // Linked chat (docs/LINKED_CHAT.md): outside messages share the chat sequence; keep the
    // newest HISTORY of both, in order. A shared chat shows every member's.
    messages.extend(match squad {
        None => crate::linked_chat::recent(app, channel, HISTORY).await?,
        Some(id) => crate::linked_chat::recent_shared(app, id, HISTORY).await?,
    });
    messages.sort_by_key(|m| m["seq"].as_i64().unwrap_or_default());
    let extra = messages.len().saturating_sub(HISTORY as usize);
    messages.drain(..extra);
    Ok(messages)
}

#[derive(Deserialize)]
pub struct Send {
    id: String,
    body: String,
    reply_to: Option<String>,
    /// Valor to pay with the message (at least 10); the channel must be able to earn.
    #[serde(default)]
    tribute: Option<i64>,
    /// Spend the channel's Engagement Valor on "highlight my message".
    #[serde(default)]
    highlight: bool,
    /// Play a Skill (docs/CROWDSYNC.md "Skills"), paid in Valor like a tribute.
    #[serde(default)]
    skill: Option<String>,
    /// A GIF the browser picked from KLIPY search; the body is then ignored.
    #[serde(default)]
    gif: Option<Gif>,
}

/// A KLIPY result as the browser received it. KLIPY's terms have media load straight from their
/// URLs, so only those URLs are stored, after checking they point at KLIPY's media host.
#[derive(Deserialize)]
pub struct Gif {
    slug: String,
    title: String,
    /// The still frame shown until the viewer hovers or taps.
    still: String,
    /// The animated version (webp).
    play: String,
    width: i32,
    height: i32,
}
const GIF_HOST: &str = "https://static.klipy.com/";
impl Gif {
    /// The stored reference, or None when it isn't a KLIPY GIF.
    fn checked(&self) -> Option<Value> {
        let url = |u: &str| {
            u.len() <= 300
                && u.strip_prefix(GIF_HOST).is_some_and(|rest| {
                    !rest.is_empty()
                        && rest
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
                })
        };
        let ok = (1..=100).contains(&self.slug.len())
            && self
                .slug
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && url(&self.still)
            && url(&self.play)
            && (1..=2000).contains(&self.width)
            && (1..=2000).contains(&self.height);
        // Every viewer draws the box at this size, so keep it chat-sized whatever the client sent.
        let scale = (400.0 / f64::from(self.width.max(self.height))).min(1.0);
        let (width, height) = (
            ((f64::from(self.width) * scale).round() as i32).max(1),
            ((f64::from(self.height) * scale).round() as i32).max(1),
        );
        ok.then(|| json!({"slug": self.slug, "still": self.still, "play": self.play, "width": width, "height": height}))
    }
    /// Alt text and the text every other reader (linked chat, apps, replay) sees.
    fn body(&self) -> String {
        let title: String = self
            .title
            .chars()
            .filter(|c| !c.is_control() && !crate::text::invisible(*c))
            .take(100)
            .collect();
        match title.trim() {
            "" => "GIF".into(),
            t => format!("GIF: {t}"),
        }
    }
}
/// The KLIPY app key, sent to browsers (KLIPY wants searches made from the viewer's device);
/// unset turns GIFs off everywhere.
fn klipy_key() -> Option<String> {
    std::env::var("KLIPY_APP_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}
/// Who may send GIFs in a channel: off, everyone, followers or subscribers.
pub(crate) async fn gif_setting(app: &App, channel: &str) -> Res<String> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce((SELECT gifs FROM chat_settings WHERE channel_id=$1),'everyone')",
    )
    .bind(channel)
    .fetch_one(&app.db)
    .await?)
}
/// What the chat composer needs: the key and who may send GIFs here, or null when nobody can.
async fn gifs(app: &App, channel: &str) -> Res<Value> {
    let who = gif_setting(app, channel).await?;
    Ok(match klipy_key() {
        Some(key) if who != "off" => json!({"key": key, "who": who}),
        _ => Value::Null,
    })
}
/// The channel's GIF setting for this sender; channel roles are exempt from the audience limit.
async fn check_gif(app: &App, channel: &str, user: &crate::auth::User) -> Res<()> {
    let who = gif_setting(app, channel).await?;
    let allowed = match who.as_str() {
        "everyone" => true,
        "off" => false,
        _ if moderation::role_of(app, channel, user).await?.is_some() => true,
        "followers" => {
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2)",
            )
            .bind(&user.id)
            .bind(channel)
            .fetch_one(&app.db)
            .await?
        }
        _ => crate::subs::active_tier(app, channel, &user.id)
            .await?
            .is_some(),
    };
    if allowed {
        return Ok(());
    }
    Err(Fail::denied(match who.as_str() {
        "off" => "GIFs are off in this chat.",
        "followers" => "GIFs here are for followers.",
        _ => "GIFs here are for subscribers.",
    }))
}

/// Persists one message to a channel's own chat and fans it out.
async fn send(
    app: &App,
    jar: &CookieJar,
    channel: &str,
    input: Send,
    squad: Option<&str>,
) -> Res<Value> {
    let user = profiles::signed_in(app, jar).await?;
    send_from(app, user, Some(channel), input, None, squad).await
}
/// Why a channel's own chat refuses posts while it is in a merged co-stream.
const SHARED_ONLY: &str = "This channel is co-streaming. Chat in the shared chat.";
/// The live merged co-stream a channel's chat has moved into, as `/api/channels/{u}/squad` shows
/// it: the channel and at least one other member still live.
async fn merged_squad(app: &App, channel: &str) -> Res<Option<String>> {
    let id: Option<String> =
        sqlx::query_scalar("SELECT squad_id FROM squad_members WHERE user_id=$1")
            .bind(channel)
            .fetch_optional(&app.db)
            .await?;
    let Some(id) = id else { return Ok(None) };
    let members = app.chat.squad(app, &id).await?;
    Ok(members
        .is_some_and(|m| m.len() > 1 && m.iter().any(|m| m == channel))
        .then_some(id))
}

/// Persists one message and fans it out. Acknowledged only after the insert commits. `origin` is
/// a MAGNet Hype lane: with a channel, a Hype message merged into that channel's chat under all of
/// its rules; without one, a message in the lane's own room (docs/MAGNET.md "Hype chat").
pub(crate) async fn send_from(
    app: &App,
    user: crate::auth::User,
    channel: Option<&str>,
    input: Send,
    origin: Option<&str>,
    squad: Option<&str>,
) -> Res<Value> {
    // A retry of a stored message is acknowledged with it, even if the room has changed since
    // (a co-stream merged, say) and a fresh send would be refused.
    let existing: Option<Row> =
        // select() contains only fixed SQL and a literal chip alias; message values are bound.
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1 AND m.author_id=$2 AND m.channel_id IS NOT DISTINCT FROM $3 AND m.squad_id IS NOT DISTINCT FROM $4 AND m.origin IS NOT DISTINCT FROM $5 AND {VISIBLE}", select())))
            .bind(&input.id)
            .bind(&user.id)
            .bind(channel)
            .bind(squad)
            .bind(origin)
            .fetch_optional(&app.db)
            .await?;
    if let Some(row) = existing {
        return Ok(visible_message(
            row.hydrated(app).await?,
            &hidden(app, Some(&user.id)).await?,
        ));
    }
    // A stale page, dock or app must not post into a room no co-stream viewer reads. MAGNet's
    // merged sends carry `origin` and `hype_merge` already picks the shared chat for them.
    if squad.is_none()
        && origin.is_none()
        && let Some(channel) = channel
        && merged_squad(app, channel).await?.is_some()
    {
        return Err(Fail::conflict(SHARED_ONLY));
    }
    // Callers resolve the person on every send, so a revoked session or new restriction takes
    // effect at once.
    let members = if let Some(id) = squad {
        let members = crate::squads::chat_context(app, id, Some(&user.id)).await?;
        if members.first().map(String::as_str) != channel {
            return Err(Fail::conflict("This co-stream has ended."));
        }
        members
    } else {
        channel.into_iter().map(str::to_owned).collect()
    };
    // Every chat the message reaches (a co-stream's members, a MAGNet Hype merge's channel):
    // closed to under-18 accounts where it's labeled mature.
    for member in &members {
        mature_gate(app, member, Some(&user.id)).await?;
    }
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(Fail::bad("Invalid message ID."));
    }
    let gif = match &input.gif {
        Some(_) if klipy_key().is_none() => {
            return Err(Fail::denied("GIFs aren't available right now."));
        }
        Some(_) if channel.is_none() || origin.is_some() => {
            return Err(Fail::field("gif", "GIFs work in a channel's own chat."));
        }
        Some(g) => Some((
            g.checked()
                .ok_or_else(|| Fail::field("gif", "Pick a GIF from the GIF search."))?,
            g.body(),
        )),
        None => None,
    };
    let body = match &gif {
        Some((_, alt)) => alt.clone(),
        None => {
            // Refuses bidi overrides and zero-width characters, which could hide a banned word
            // or reorder the line on screen.
            let body = crate::text::clean(&input.body, "body", true)?;
            if crate::text::stacked(&body) {
                return Err(Fail::field(
                    "body",
                    "Remove the stacked accent marks and try again.",
                ));
            }
            body
        }
    };
    let body = body.as_str();
    let length = body.chars().count();
    if length == 0 || length > 500 {
        return Err(Fail::field("body", "Messages are 1–500 characters."));
    }
    if body.matches('\n').count() > 4 {
        return Err(Fail::field("body", "Use at most four line breaks."));
    }
    let standing: Option<(bool, bool)> =
        sqlx::query_as("SELECT email_verified, eligible FROM channel_users WHERE id=$1")
            .bind(&user.id)
            .fetch_optional(&app.db)
            .await?;
    match standing {
        Some((false, _)) => return Err(Fail::denied("Verify your email address to chat.")),
        Some((true, true)) => {}
        _ => return Err(Fail::denied("Your account can't chat right now.")),
    }
    // Before slow mode and rate limits, so a refused GIF costs the sender nothing.
    if gif.is_some() {
        for member in &members {
            check_gif(app, member, &user).await?;
        }
    }
    // Each chat's slow-mode turn, taken only once everything else has passed.
    let mut slows: Vec<(&str, i32)> = Vec::new();
    let role = match channel {
        Some(channel) => {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1))")
                .bind(channel).bind(&user.id).fetch_one(&app.db).await?;
            if blocked {
                return Err(Fail::denied("You can't chat in this channel."));
            }
            let checked = moderation::check_send(app, channel, &user, body).await?;
            slows.push((channel, checked.slow));
            checked.role.map(|r| r.name())
        }
        None if input.reply_to.is_some() => {
            return Err(Fail::field(
                "reply_to",
                "Replies work inside a channel's chat.",
            ));
        }
        None => None,
    };
    if input.highlight && (channel.is_none() || origin.is_some()) {
        return Err(Fail::field(
            "highlight",
            "Highlights work in a channel's own chat.",
        ));
    }
    // Who a tribute pays: the channel, or in a merged co-stream every member who can earn,
    // split equally (docs/SUPPORT.md "Co-streams").
    let mut earners: Vec<String> = Vec::new();
    let skill = match (&input.skill, channel) {
        (Some(_), _) if input.tribute.is_some() => {
            return Err(Fail::field("skill", "A Skill is its own payment."));
        }
        (Some(id), Some(channel)) if origin.is_none() && squad.is_none() => {
            Some(crate::skills::check(app, channel, id).await?)
        }
        (Some(_), _) => {
            return Err(Fail::field("skill", "Skills play in a channel's own chat."));
        }
        (None, _) => None,
    };
    let tribute = input.tribute.or(skill.as_ref().map(|s| s.valor));
    if let Some(valor) = tribute {
        if channel.is_none() || origin.is_some() {
            return Err(Fail::field("tribute", "Tributes go to a channel's chat."));
        }
        if !(10..=1_000_000).contains(&valor) {
            return Err(Fail::field("tribute", "A tribute is at least 10 Valor."));
        }
        if members.contains(&user.id) {
            return Err(Fail::field("tribute", "You can't pay tribute to yourself."));
        }
        let mut db = app.db.acquire().await?;
        for member in &members {
            if crate::support::can_earn(&mut db, member).await? {
                earners.push(member.clone());
            }
        }
        if earners.is_empty() {
            return Err(Fail::field(
                "tribute",
                "This channel can't receive tributes yet.",
            ));
        }
    }
    // Flood protection for small channels: Hype senders get one message every 3 seconds.
    if let Some(lane) = origin {
        let room = channel.unwrap_or(lane);
        sec::reserve(app, vec![format!("hype-slow:{room}:{}", user.id)], 1, 3).await?;
    }
    sec::reserve(app, vec![format!("chat-second:{}", user.id)], 2, 1).await?;
    sec::reserve(app, vec![format!("chat-ten:{}", user.id)], 20, 10).await?;
    let hidden = hidden(app, Some(&user.id)).await?;
    let role = if squad.is_some() {
        for member in members.iter().skip(1) {
            slows.push((
                member,
                moderation::check_send(app, member, &user, body).await?.slow,
            ));
        }
        crate::squads::chat_role(app, &members, &user)
            .await?
            .map(|r| r.name())
    } else {
        role
    };
    let reserved = moderation::reserve_slow(app, &user.id, &slows).await?;
    // A refusal from here on (a gone reply target, an ID clash, too little Valor) hands the
    // slow-mode turns back.
    let stored = async {
        let mut tx = app.db.begin().await?;
        if let Some(id) = squad {
            crate::squads::lock_chat(&mut tx, id, &members).await?;
        }
        if let (Some(reply), Some(channel)) = (&input.reply_to, channel) {
            // Lock against deletion while accepting the reply. Later reads always join the current body.
            let author: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND m.squad_id IS NOT DISTINCT FROM $3 AND {VISIBLE} FOR SHARE")))
                .bind(reply).bind(channel).bind(squad).fetch_optional(&mut *tx).await?;
            if author.is_none_or(|a| hidden.contains(&a)) {
                return Err(Fail::field(
                    "reply_to",
                    "Reply to a visible message in this channel.",
                ));
            }
        }
        let mentions: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM channel_users WHERE lower(username)=ANY($1) AND eligible",
        )
        .bind(mention_names(body))
        .fetch_all(&mut *tx)
        .await?;
        // A shared chat line credits one member channel, so rewards never multiply across members;
        // MAGNet's merged sends earn nothing there, as before.
        let credit = match (channel, squad) {
            (Some(_), Some(_)) if origin.is_none() => {
                Some(crate::engagement::credit(&mut tx, &members, &user.id).await?)
            }
            (Some(channel), None) => Some(channel.to_owned()),
            _ => None,
        };
        let inserted = sqlx::query("INSERT INTO chat_messages(id,channel_id,author_id,body,reply_to,mention_ids,role,origin,squad_id,tribute,highlighted,skill,gif,credit_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT(id) DO NOTHING")
            .bind(&input.id).bind(channel).bind(&user.id).bind(body).bind(&input.reply_to).bind(mentions).bind(role).bind(origin).bind(squad).bind(tribute.map(|v| v as i32)).bind(input.highlight).bind(skill.as_ref().map(|s| s.shown.as_str())).bind(gif.as_ref().map(|(g, _)| g.clone())).bind(credit.as_deref().filter(|_| squad.is_some())).execute(&mut *tx).await?.rows_affected();
        if inserted == 0 {
            // A retry of the author's own message that was removed meanwhile.
            let mine: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM chat_messages WHERE id=$1 AND author_id=$2)",
            )
            .bind(&input.id)
            .bind(&user.id)
            .fetch_one(&mut *tx)
            .await?;
            return Err(Fail::conflict(if mine {
                "That message was removed."
            } else {
                "That message ID is already in use."
            }));
        }
        if let (Some(valor), Some(channel)) = (tribute, channel) {
            // The message and the Valor move commit together; the lock serializes the buyer's spends.
            let wallet = format!("valor:{}", user.id);
            crate::ledger::lock(&mut tx, &wallet).await?;
            let balance = crate::ledger::balance(&mut tx, &wallet, "valor").await?;
            if balance < 0 {
                return Err(Fail::field(
                    "tribute",
                    "Your Valor is locked until a payment problem is settled.",
                ));
            }
            if balance < valor {
                return Err(Fail::field("tribute", "You don't have enough Valor."));
            }
            let earned = valor * crate::ledger::EARN_PER_VALOR;
            let shares: Vec<(String, i64)> = crate::support::split_equally(&earners, earned)
                .into_iter()
                .map(|(m, s)| (format!("usd:earnings:{m}"), s))
                .collect();
            let mut entries = vec![
                (wallet.as_str(), "valor", -valor),
                ("valor:spent", "valor", valor),
                ("usd:platform", "usd", -earned),
            ];
            entries.extend(shares.iter().map(|(a, s)| (a.as_str(), "usd", *s)));
            crate::ledger::post(
                &mut tx,
                "tribute",
                &format!("tribute:{}", input.id),
                json!({"message": input.id, "channel": channel, "earners": earners, "from": user.id, "valor": valor, "skill": skill.as_ref().map(|s| s.id.as_str())}),
                &entries,
            )
            .await?;
            // The sender counts toward each paid channel's faction they're watching, once a day;
            // a Skill never does (docs/FACTIONS.md "Influence", docs/CROWDSYNC.md "Skills").
            if skill.is_none() {
                let watched = crate::factions::watching(&mut tx, &earners, &user.id).await?;
                let settled = format!("tribute:{}", input.id);
                crate::factions::support(app, &mut tx, &watched, &user.id, &settled).await?;
            }
            crate::events::emit(
                &mut tx,
                channel,
                "tributes",
                json!({"user": user.username, "valor": valor, "message": body}),
            )
            .await?;
            if let Some(skill) = &skill {
                crate::events::emit(
                    &mut tx,
                    channel,
                    "skills",
                    json!({"user": user.username, "skill": skill.shown, "valor": valor}),
                )
                .await?;
            }
        }
        // A moderator's counter command (`!deaths`) changed a counter; viewers are told after commit.
        let mut counters = None;
        let mut rallied = false;
        if let Some(credit) = &credit {
            if input.highlight {
                crate::engagement::highlight(&mut tx, credit, &user.id, &input.id).await?;
            }
            crate::engagement::chatted(&mut tx, &app.config.engagement, credit, &user.id).await?;
            crate::factions::chat(app, &mut tx, credit, &user.id, &input.id).await?;
            // Every member's Plays vote: only a Plays channel counts it.
            for member in &members {
                crate::plays::chat_vote(&mut tx, member, &user, body).await?;
            }
            let mut command = body.splitn(2, char::is_whitespace);
            if origin.is_none() && command.next() == Some("!marker") {
                let note = command.next().unwrap_or("");
                for member in &members {
                    if moderation::role_of(app, member, &user).await?.is_some() {
                        crate::videos::marker(&mut tx, member, note, Some(&input.id)).await?;
                    }
                }
            }
            counters = crate::crowd::chat_command(app, &mut tx, &members, &user, body).await?;
            // Surge counts each real viewer once a minute; "!rally" also rallies for their faction.
            crate::surge::participated(&mut tx, credit, &user.id).await?;
            rallied = body.eq_ignore_ascii_case("!rally")
                && crate::surge::rally_in(&mut tx, credit, &user.id).await?;
        }
        tx.commit().await?;
        Ok::<_, Fail>((credit, counters, rallied))
    }
    .await;
    let (credit, counters, rallied) = match stored {
        Ok(done) => done,
        Err(e) => {
            let _ = sec::release(app, reserved).await;
            return Err(e);
        }
    };
    if let Some(owner) = &counters {
        crate::crowd::publish_counters(app, owner).await?;
    }
    if let Some(credit) = &credit {
        if rallied {
            crate::surge::publish_rally(app, credit).await?;
        }
        if let Some(skill) = &skill {
            crate::skills::played(app, credit, &user, skill).await?;
        }
    }
    // Combos count the whole room (a shared chat's chatters credit different members) and play
    // on the member whose emote it is, host first.
    if let Some(room) = squad.or(channel).filter(|_| origin.is_none()) {
        crate::surge::combo(app, room, &members, &user.id, body).await?;
    }
    // select() contains only fixed SQL and a literal chip alias; message values are bound.
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE m.id=$1", select())))
        .bind(&input.id)
        .fetch_one(&app.db)
        .await?;
    let (seq, author) = (row.seq, row.author_id.clone());
    let message = row.hydrated(app).await?;
    let event = json!({"type":"message","message":message});
    if let Some(channel) = squad.or(channel) {
        app.chat.publish(channel, Some(&author), seq, event.clone());
    }
    if let Some(lane) = origin {
        app.chat
            .publish(&format!("magnet:{lane}"), Some(&author), 0, event);
    }
    // Custom !commands get the channel bot's reply after the message itself (docs/COMMUNITY.md);
    // in shared chat, from the host's commands first, into the shared room.
    if channel.is_some() && origin.is_none() {
        crate::commands::respond(app, &members, squad, &user, body).await;
    }
    Ok(visible_message(message, &hidden))
}

pub async fn read(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
    let (hidden, _) = hidden_for(
        &app,
        viewer.as_ref().map(|v| v.id.as_str()),
        std::slice::from_ref(&channel),
    )
    .await?;
    let fade: i32 = sqlx::query_scalar(
        "SELECT coalesce((SELECT overlay_fade_seconds FROM chat_settings WHERE channel_id=$1),30)",
    )
    .bind(&channel)
    .fetch_one(&app.db)
    .await?;
    let rules = app.chat.rules(&app, &channel).await?;
    Ok(Json(
        json!({"now": Utc::now(), "overlay_fade_seconds": fade, "allow_signatures": rules.allow_signatures, "gifs": gifs(&app, &channel).await?, "messages": history(&app, &channel, &hidden, None).await?, "pinned":pinned(&app, &channel, &hidden).await?, "emotes":rules.emotes, "followers_only_until":rules.followers_only, "subs_only":rules.subs_only}),
    ))
}
pub async fn post(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Send>,
) -> Res<Response> {
    let channel = channel(&app, &name).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
    match send(&app, &jar, &channel, input, None).await {
        Ok(message) => Ok(Json(json!({"message": message})).into_response()),
        // Names the shared chat, so the client can switch rooms.
        Err(fail) if fail.message == SHARED_ONLY => Ok((
            fail.status,
            Json(json!({"error": fail.message, "squad": merged_squad(&app, &channel).await?})),
        )
            .into_response()),
        Err(fail) => Err(fail),
    }
}
/// Chat is closed to under-18 accounts in a channel labeled mature.
async fn mature_gate(app: &App, channel: &str, viewer: Option<&str>) -> Res<()> {
    if crate::streams::mature_blocked(&mut *app.db.acquire().await?, channel, viewer).await? {
        return Err(Fail::denied(
            "This channel is labeled mature, so its chat isn't available on your account.",
        ));
    }
    Ok(())
}

/// A shared chat's members (host first) for reading: one member labeled mature closes it to
/// under-18 accounts, as sending already does.
async fn squad_members(app: &App, id: &str, viewer: Option<&str>) -> Res<Vec<String>> {
    let members = crate::squads::chat_context(app, id, viewer).await?;
    for member in &members {
        mature_gate(app, member, viewer).await?;
    }
    Ok(members)
}
pub async fn squad_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let viewer = profiles::viewer(&app, &jar).await?;
    let members = squad_members(&app, &id, viewer.as_ref().map(|u| u.id.as_str())).await?;
    let channel = members.first().ok_or_else(Fail::missing)?;
    let (hidden, _) = hidden_for(&app, viewer.as_ref().map(|u| u.id.as_str()), &members).await?;
    let names = member_names(&app, &members).await?;
    let rules = squad_rules(&app, &names).await?;
    let credit = match &viewer {
        Some(v) => credit_name(&app, &members, &names, &v.id).await?,
        None => None,
    };
    Ok(Json(
        json!({"now":Utc::now(),"messages":history(&app,channel,&hidden,Some(&id)).await?,"pinned":null,"emotes":rules.emotes,"allow_signatures":rules.allow_signatures,"followers_only_until":rules.followers_only,"subs_only":rules.subs_only,"credit":credit}),
    ))
}
/// The member channel a shared-chat viewer's lines credit, by username: whose rewards the dock
/// offers.
async fn credit_name(
    app: &App,
    members: &[String],
    names: &[(String, String)],
    viewer: &str,
) -> Res<Option<String>> {
    let id = crate::engagement::credit(&mut *app.db.acquire().await?, members, viewer).await?;
    Ok(names.iter().find(|n| n.0 == id).map(|n| n.1.clone()))
}
/// Members' usernames, in member order.
async fn member_names(app: &App, members: &[String]) -> Res<Vec<(String, String)>> {
    let mut names: Vec<(String, String)> =
        sqlx::query_as("SELECT id,username FROM users WHERE id=ANY($1)")
            .bind(members)
            .fetch_all(&app.db)
            .await?;
    names.sort_by_key(|(id, _)| members.iter().position(|m| m == id));
    Ok(names)
}
/// A shared chat's rules: every member's emotes (host first, the first of each code wins, each
/// tagged with its channel), subscribers-only while any member's is, followers-only until the
/// latest member's ends, and the host's signature setting.
async fn squad_rules(app: &App, members: &[(String, String)]) -> Res<Rules> {
    let mut united = Rules::default();
    let mut codes = HashSet::new();
    for (n, (id, name)) in members.iter().enumerate() {
        let rules = app.chat.rules(app, id).await?;
        if n == 0 {
            united.allow_signatures = rules.allow_signatures;
        }
        united.subs_only |= rules.subs_only;
        united.followers_only = united.followers_only.max(rules.followers_only);
        for mut emote in rules.emotes {
            if codes.insert(emote["code"].as_str().unwrap_or_default().to_owned()) {
                emote["channel"] = json!(name);
                united.emotes.push(emote);
            }
        }
    }
    Ok(united)
}
/// Frames for what changed between two readings of a chat's rules. An `emotes` event always gets
/// its frame (with `outside` passed on), so clients re-resolve signature emotes too.
fn rule_changes(old: &Rules, new: &Rules, emotes_event: Option<&Value>) -> Vec<Value> {
    let mut frames = Vec::new();
    if emotes_event.is_some()
        || old.emotes != new.emotes
        || old.allow_signatures != new.allow_signatures
    {
        let mut frame =
            json!({"type":"emotes","emotes":new.emotes,"allow_signatures":new.allow_signatures});
        if let Some(outside) = emotes_event.and_then(|e| e.get("outside")) {
            frame["outside"] = outside.clone();
        }
        frames.push(frame);
    }
    if old.subs_only != new.subs_only {
        frames.push(json!({"type":"subs_only","on":new.subs_only}));
    }
    if old.followers_only != new.followers_only {
        frames.push(json!({"type":"protect","until":new.followers_only}));
    }
    frames
}
/// Whether any of these channels other than the viewer's own is labeled mature now.
async fn mature_any(app: &App, channels: &[String], viewer: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM stream_settings WHERE mature AND owner_id=ANY($1) AND owner_id<>$2)")
        .bind(channels)
        .bind(viewer)
        .fetch_one(&app.db)
        .await?)
}
/// A signed-in viewer under 18, who is re-checked against mature labels while connected. Age only
/// loosens the gate, so it's read once; an unreadable age counts as under 18.
async fn minor(app: &App, viewer: Option<&str>) -> bool {
    let Some(viewer) = viewer else { return false };
    match app.db.acquire().await {
        Ok(mut db) => !crate::auth::is_adult(&mut db, viewer)
            .await
            .unwrap_or(false),
        Err(_) => true,
    }
}
pub async fn squad_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let members = crate::squads::chat_context(&app, &id, Some(&user.id)).await?;
    let channel = members.first().ok_or_else(Fail::missing)?;
    Ok(Json(
        json!({"message":send(&app,&jar,channel,input,Some(&id)).await?}),
    ))
}
/// Called only after the squad module authorizes a current shared-chat moderator. Mirrors a
/// channel delete: staff delete anything; a member streamer anything but another member
/// streamer's lines; a moderator nothing protected in any member channel. Logged in every member's
/// channel.
pub async fn remove_shared(
    app: &App,
    squad: &str,
    id: &str,
    user: &crate::auth::User,
    role: moderation::Role,
    members: &[String],
    reason: &str,
) -> Res<()> {
    let reason = moderation::delete_reason(reason)?;
    let (author, origin): (String, Option<String>) =
        sqlx::query_as("SELECT author_id,origin FROM chat_messages WHERE id=$1 AND squad_id=$2")
            .bind(id)
            .bind(squad)
            .fetch_optional(&app.db)
            .await?
            .ok_or_else(Fail::missing)?;
    if author != user.id {
        let refused = match role {
            moderation::Role::Staff => false,
            moderation::Role::Owner => members.contains(&author),
            moderation::Role::Moderator => {
                let mut refused = false;
                for channel in members {
                    if moderation::protected(app, channel, &user.id, &author).await? {
                        refused = true;
                        break;
                    }
                }
                refused
            }
        };
        if refused {
            return Err(Fail::denied("You can't delete this message."));
        }
    }
    let mut tx = app.db.begin().await?;
    crate::squads::lock_chat(&mut tx, squad, members).await?;
    let changed=sqlx::query("UPDATE chat_messages SET deleted_at=now() WHERE id=$1 AND squad_id=$2 AND deleted_at IS NULL").bind(id).bind(squad).execute(&mut *tx).await?.rows_affected();
    if changed > 0 {
        crate::safety::audit(
            &mut tx,
            Some(&user.id),
            "delete_message",
            "squad",
            squad,
            &[],
            &reason,
            json!({"message":id}),
            false,
        )
        .await?;
        for channel in members {
            moderation::log(
                &mut tx,
                channel,
                &user.id,
                role,
                "delete_message",
                Some(&author),
                Some(id),
                json!({"squad": squad}),
                &reason,
            )
            .await?;
        }
    }
    tx.commit().await?;
    if changed > 0 {
        publish_delete(app, Some(squad), origin.as_deref(), id);
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Pin {
    message_id: Option<String>,
    #[serde(default)]
    reason: String,
}
/// A null message unpins. An upsert makes concurrent replacements leave exactly one pin.
pub async fn pin(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Pin>,
) -> Res<Json<Value>> {
    let channel = channel(&app, &name).await?;
    let (user, role) = moderation::actor(&app, &jar, &channel).await?;
    if !matches!(role, moderation::Role::Owner | moderation::Role::Moderator) {
        return Err(Fail::denied(
            "Only the channel owner or an appointed moderator can pin messages.",
        ));
    }
    let reason = moderation::optional_reason(&input.reason, "Pin changed")?;
    let hidden = hidden(&app, Some(&user.id)).await?;
    let mut tx = app.db.begin().await?;
    if let Some(id) = &input.message_id {
        let author: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT m.author_id FROM chat_messages m WHERE m.id=$1 AND m.channel_id=$2 AND m.squad_id IS NULL AND {VISIBLE} FOR UPDATE"
        ))).bind(id).bind(&channel).fetch_optional(&mut *tx).await?;
        if author.is_none_or(|a| hidden.contains(&a)) {
            return Err(Fail::missing());
        }
        sqlx::query("INSERT INTO chat_pins(channel_id,message_id) VALUES($1,$2) ON CONFLICT(channel_id) DO UPDATE SET message_id=EXCLUDED.message_id")
            .bind(&channel).bind(id).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM chat_pins WHERE channel_id=$1")
            .bind(&channel)
            .execute(&mut *tx)
            .await?;
    }
    moderation::log(
        &mut tx,
        &channel,
        &user.id,
        role,
        if input.message_id.is_some() {
            "pin_message"
        } else {
            "unpin_message"
        },
        None,
        input.message_id.as_deref(),
        json!({}),
        &reason,
    )
    .await?;
    tx.commit().await?;
    // One read for every socket, after commit; sockets filter it for their viewer.
    let pin = app.chat.repin(&app, &channel).await;
    app.chat.publish(&channel, None, 0, json!({"type":"pin"}));
    Ok(Json(json!({"pinned":pin_view(&app.chat, &pin?, &hidden)})))
}

#[derive(Deserialize)]
pub struct Join {
    #[serde(default)]
    channel: String,
    squad: Option<String>,
}
/// Same-origin socket on the session cookie (never a token in the URL); one channel per connection.
pub async fn socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Query(join): Query<Join>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let ip = network(sec::client_ip(&app, peer, &headers));
    sec::reserve(&app, vec![format!("chat-connect:{ip}")], 30, 60).await?;
    // A shared chat's members (host first), or the one channel.
    let members = if let Some(id) = &join.squad {
        let viewer = profiles::viewer(&app, &jar).await?;
        squad_members(&app, id, viewer.as_ref().map(|u| u.id.as_str())).await?
    } else {
        let channel = channel(&app, &join.channel).await?;
        let viewer = profiles::viewer(&app, &jar).await?;
        mature_gate(&app, &channel, viewer.as_ref().map(|v| v.id.as_str())).await?;
        vec![channel]
    };
    let channel = members.first().cloned().ok_or_else(Fail::missing)?;
    let slot = app.chat.open(ip)?;
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| session(app, jar, channel, members, join.squad, slot, ws)))
}

/// Member-channel events a shared chat passes on, marked with their channel (`source`): live polls,
/// counters, rally, surge and board updates. Raids aren't: co-stream players don't follow them.
const SHARED_EVENTS: [&str; 9] = [
    "board",
    "board_input",
    "board_effect",
    "board_state",
    "board_result",
    "poll",
    "counters",
    "rally",
    "surge",
];
/// The rules a socket shows: its channel's, or a shared chat's members' united.
async fn rules_for(app: &App, channel: &str, names: Option<&[(String, String)]>) -> Res<Rules> {
    match names {
        Some(names) => squad_rules(app, names).await,
        None => app.chat.rules(app, channel).await,
    }
}
async fn session(
    app: App,
    jar: CookieJar,
    channel: String,
    mut members: Vec<String>,
    squad: Option<String>,
    _slot: SocketSlot,
    mut ws: WebSocket,
) {
    let room = squad.as_deref().unwrap_or(&channel);
    // Its room and, in shared chat, each member channel's own (their rules and live panels).
    let rooms_of = |members: &[String]| -> Vec<String> {
        let mut rooms = vec![room.to_string()];
        if squad.is_some() {
            rooms.extend(members.iter().cloned());
        }
        rooms
    };
    // Subscribe before reading history so nothing posted in between is missed.
    let mut events = Rooms::default();
    events.set(&app.chat, &rooms_of(&members));
    let viewer = profiles::viewer(&app, &jar).await.ok().flatten();
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let minor = minor(&app, viewer_id.as_deref()).await;
    // A shared chat's members with their usernames, host first; None for a channel's own chat.
    let mut names = match &squad {
        Some(_) => match member_names(&app, &members).await {
            Ok(names) => Some(names),
            Err(_) => return,
        },
        None => None,
    };
    let mut blocks = Hidden::new(members.clone());
    let Ok(hidden) = blocks.current(&app, viewer_id.as_deref()).await else {
        return;
    };
    let Ok(snapshot) = history(&app, &channel, hidden, squad.as_deref()).await else {
        return;
    };
    let pin = match &squad {
        Some(_) => Value::Null,
        None => match pinned(&app, &channel, hidden).await {
            Ok(pin) => pin,
            Err(_) => return,
        },
    };
    // Messages the snapshot already holds aren't sent again. Not a seq cursor: a slow send (a
    // tribute) can commit after a later seq, and must still reach this socket.
    let mut seen: HashSet<String> = snapshot
        .iter()
        .filter_map(|m| m["id"].as_str().map(String::from))
        .collect();
    let joined = Instant::now();
    let Ok(mut rules) = rules_for(&app, &channel, names.as_deref()).await else {
        return;
    };
    let Ok(gifs) = gifs(&app, &channel).await else {
        return;
    };
    // Read once per connection, like the rest of the snapshot.
    let credit = match (&names, &viewer_id) {
        (Some(names), Some(viewer)) => match credit_name(&app, &members, names, viewer).await {
            Ok(credit) => credit,
            Err(_) => return,
        },
        _ => None,
    };
    let first = json!({"type":"snapshot","now":Utc::now(),"messages":snapshot,"pinned":pin,"emotes":rules.emotes,"followers_only_until":rules.followers_only,"subs_only":rules.subs_only,"allow_signatures":rules.allow_signatures,"gifs":gifs,"credit":credit}).to_string();
    if ws.send(Message::Text(first.into())).await.is_err() {
        return;
    }
    // Also catches quarantine, appeals and account removals performed by the worker. Rules are
    // shared, so this is one read per channel every five seconds, not one per socket.
    let mut refresh = tokio::time::interval(Duration::from_secs(5));
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ping = pings();
    let mut alive = Instant::now();
    // At most 10 frames a second; the per-person send limits sit behind several queries.
    let (mut window, mut frames) = (Instant::now(), 0u32);
    // A shared chat's roster is shared by its sockets and read at most once a second, so an ended
    // co-stream closes within a second. The viewer's own access (bans, restrictions, blocks) is
    // re-checked only when the roster or the access generation moves, or every 30 seconds; the
    // first check runs a second in, covering anything between connecting and subscribing.
    let mut access = tokio::time::interval(Duration::from_secs(1));
    access.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (mut checked, mut checked_at) = (None, Instant::now());
    // The merged co-stream this channel's chat moved into, as last told to the client.
    let mut moved: Option<String> = None;
    loop {
        tokio::select! {
            _ = refresh.tick() => {
                // Any duplicate of the snapshot has arrived by now.
                if !seen.is_empty() && joined.elapsed() > Duration::from_secs(60) {
                    seen = HashSet::new();
                }
                blocks.recheck();
                // A chat labeled mature while an under-18 viewer has it open closes for them, for
                // good: clients don't reconnect on 4003.
                if minor
                    && let Some(v) = viewer_id.as_deref()
                    && mature_any(&app, &members, v).await.unwrap_or(false)
                {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4003, reason: "mature".into() }))).await;
                    return;
                }
                // A channel's own chat that merges into a co-stream, or stops, tells the client to
                // switch rooms (a shared chat that ends closes with 4003 instead).
                if squad.is_none()
                    && let Ok(now) = app.chat.merged(&app, &channel).await
                    && now != moved
                {
                    moved = now;
                    let frame = json!({"type":"room","squad":moved});
                    if ws.send(Message::Text(frame.to_string().into())).await.is_err() { return; }
                }
                let Ok(next) = rules_for(&app, &channel, names.as_deref()).await else { continue; };
                for frame in rule_changes(&rules, &next, None) {
                    if ws.send(Message::Text(frame.to_string().into())).await.is_err() { return; }
                }
                rules = next;
            }
            _ = access.tick(), if squad.is_some() => {
                let Some(id) = squad.as_deref() else { continue; };
                let generation = app.chat.access_generation();
                // A transient error keeps the socket open; the next tick tries again.
                // Ended or barred closes with 4003, which clients treat as final (one read then
                // shows why) instead of reconnecting to a socket that would be refused.
                let current = match app.chat.squad(&app, id).await {
                    Ok(Some(current)) => current,
                    Ok(None) => {
                        let _ = ws.send(Message::Close(Some(CloseFrame { code: 4003, reason: "ended".into() }))).await;
                        return;
                    }
                    Err(_) => continue,
                };
                if current == members && checked == Some(generation) && checked_at.elapsed() < Duration::from_secs(30) {
                    continue;
                }
                if let Some(v) = viewer_id.as_deref() {
                    match crate::squads::barred(&app.db, id, v, &current).await {
                        Ok(None) => {}
                        Ok(Some(_)) => {
                            let _ = ws.send(Message::Close(Some(CloseFrame { code: 4003, reason: "barred".into() }))).await;
                            return;
                        }
                        Err(_) => continue,
                    }
                }
                (checked, checked_at) = (Some(generation), Instant::now());
                if current != members {
                    let Ok(next_names) = member_names(&app, &current).await else { continue; };
                    blocks.rescope(current.clone());
                    events.set(&app.chat, &rooms_of(&current));
                    (members, names) = (current, Some(next_names));
                    // The emotes and the banner follow who's in it.
                    let Ok(next) = rules_for(&app, &channel, names.as_deref()).await else { continue; };
                    for frame in rule_changes(&rules, &next, None) {
                        if ws.send(Message::Text(frame.to_string().into())).await.is_err() { return; }
                    }
                    rules = next;
                }
            }
            _ = ping.tick() => {
                if alive.elapsed() > SILENT || ws.send(Message::Ping(Default::default())).await.is_err() { return; }
            }
            event = events.recv() => match event {
                Ok(e) => {
                    let kind = e.payload["type"].as_str().unwrap_or_default();
                    // In shared chat, a member channel's own events: its rules and live panels.
                    let source = names
                        .as_ref()
                        .filter(|_| e.channel != room)
                        .and_then(|names| names.iter().find(|(id, _)| *id == e.channel))
                        .map(|(_, name)| name.clone());
                    if matches!(kind, "emotes" | "subs_only" | "protect") {
                        let Ok(next) = rules_for(&app, &channel, names.as_deref()).await else { continue; };
                        for frame in rule_changes(&rules, &next, (kind == "emotes").then_some(&e.payload)) {
                            if ws.send(Message::Text(frame.to_string().into())).await.is_err() { return; }
                        }
                        rules = next;
                        continue;
                    }
                    // A member's linked-platform lines (not its bot's) and their hides join the
                    // shared chat (outside IDs are `platform:id`); MAGNet never hears them here.
                    let outside = source.is_some() && match kind {
                        "message" => !e.payload["message"]["outside"].is_null() && e.payload["message"]["outside"]["platform"] != "bot",
                        "delete" => e.payload["id"].as_str().is_some_and(|id| id.contains(':')),
                        _ => false,
                    };
                    if source.is_some() && !outside && !SHARED_EVENTS.contains(&kind) { continue; }
                    if e.seq != 0 && e.payload["message"]["id"].as_str().is_some_and(|id| seen.contains(id)) { continue; }
                    let Ok(hidden) = blocks.current(&app, viewer_id.as_deref()).await else { return; };
                    if e.author.as_ref().is_some_and(|a| hidden.contains(a)) { continue; }
                    let mut payload = e.payload.clone();
                    if let Some(source) = source {
                        if !outside {
                            payload["source"] = json!(source);
                        } else if kind == "message" {
                            payload["message"]["outside"]["channel"] = json!(source);
                        }
                    } else if kind == "pin" {
                        payload["pinned"] = match app.chat.pin(&channel) {
                            Some(pin) => pin_view(&app.chat, &pin, hidden),
                            None => match pinned(&app, &channel, hidden).await {
                                Ok(pin) => pin,
                                Err(_) => return,
                            },
                        };
                    } else if kind == "message" && payload["message"]["outside"].is_null() {
                        // Outside messages and the bot's lines aren't in chat_messages and have no
                        // S.V.E.R author; they go out as published (a later hide is its own event).
                        let Some(message) = fresh(&app.chat, &payload, hidden) else { continue; };
                        payload["message"] = message;
                    }
                    if ws.send(Message::Text(payload.to_string().into())).await.is_err() {
                        return;
                    }
                }
                // A reader that fell behind reconnects and reloads rather than buffering without bound.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4000, reason: "resync".into() }))).await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    alive = Instant::now();
                    if window.elapsed() >= Duration::from_secs(1) {
                        (window, frames) = (Instant::now(), 0);
                    }
                    frames += 1;
                    let reply = match serde_json::from_str::<Send>(&text) {
                        Ok(_) if frames > 10 => json!({"type":"error","message":"You're sending too fast.","retry":1}),
                        Ok(input) => {
                            let id = input.id.clone();
                            match send(&app, &jar, &channel, input, squad.as_deref()).await {
                                Ok(message) => json!({"type":"ack","id":id,"message":message}),
                                Err(fail) => json!({"type":"error","id":id,"message":fail.message,"retry":fail.retry}),
                            }
                        }
                        Err(_) => json!({"type":"error","message":"Invalid chat command."}),
                    };
                    if ws.send(Message::Text(reply.to_string().into())).await.is_err() {
                        return;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => alive = Instant::now(),
            },
        }
    }
}

/// A published chat message as this recipient sees it, without a database read: the payload was
/// read after its insert committed, and events reach every socket in publish order, so a later
/// delete follows it. None when its delete was published first; a quote of a deleted message or
/// of someone the recipient blocks is blanked.
pub(crate) fn fresh(hub: &Hub, payload: &Value, hidden: &HashSet<String>) -> Option<Value> {
    let mut message = payload["message"].clone();
    if hub.deleted(message["id"].as_str().unwrap_or("")) {
        return None;
    }
    if message["reply"]["id"]
        .as_str()
        .is_some_and(|id| hub.deleted(id))
    {
        message["reply"]["body"] = Value::Null;
        message["reply"]["username"] = Value::Null;
    }
    Some(visible_message(message, hidden))
}
/// A message sent by an app as the person (events socket, `chat:write`): chat only. Money is never
/// reachable through an app, so tributes, Skills and highlights are refused, and a banned account
/// can't post.
pub(crate) async fn send_as_app(
    app: &App,
    user: &str,
    channel: &str,
    params: &Value,
) -> Res<Value> {
    let input: Send = serde_json::from_value(params.clone())
        .map_err(|_| Fail::bad("Send {\"channel\",\"id\",\"body\"}."))?;
    if input.tribute.is_some() || input.skill.is_some() || input.highlight {
        return Err(Fail::denied("Apps can't spend Valor."));
    }
    let person = crate::devapps::person(app, user)
        .await?
        .ok_or_else(|| Fail::denied("Your account can't chat right now."))?;
    if crate::streams::mature_blocked(&mut *app.db.acquire().await?, channel, Some(user)).await? {
        return Err(Fail::denied(
            "This channel is labeled mature, so its chat isn't available on your account.",
        ));
    }
    send_from(app, person, Some(channel), input, None, None).await
}

/// What a Hype lane's chat is merged with right now: the featured channel's chat, or a merged
/// co-stream's shared chat (stored under its host), and since when.
struct Merge {
    channel: String,
    squad: Option<String>,
    /// Every streamer whose chat rules and bans apply.
    owners: Vec<String>,
    since: DateTime<Utc>,
}
impl Merge {
    /// The broadcast room its messages arrive on.
    fn room(&self) -> &str {
        self.squad.as_deref().unwrap_or(&self.channel)
    }
}
/// Merging stops when MAGNet moves on, when the lane stops, or when a streamer whose chat it would
/// join turns chat merging off.
async fn hype_merge(app: &App, lane: &str) -> Res<Option<Merge>> {
    let featured: Option<(String, DateTime<Utc>)> = sqlx::query_as("SELECT b.owner_id,l.current_since FROM magnet_lanes l JOIN broadcasts b ON b.id=l.current_broadcast WHERE l.id=$1 AND l.enabled AND l.current_since IS NOT NULL")
        .bind(lane)
        .fetch_optional(&app.db)
        .await?;
    let Some((owner, since)) = featured else {
        return Ok(None);
    };
    let squad = crate::squads::active_of(&mut *app.db.acquire().await?, &owner)
        .await?
        .filter(|q| q.mode == "MERGED");
    let merge = match squad {
        Some(q) => Merge {
            channel: q.members[0].0.clone(),
            squad: Some(q.id),
            owners: q.members.into_iter().map(|m| m.0).collect(),
            since,
        },
        None => Merge {
            channel: owner.clone(),
            squad: None,
            owners: vec![owner],
            since,
        },
    };
    let off: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM magnet_settings WHERE user_id=ANY($1) AND NOT chat_merge)",
    )
    .bind(&merge.owners)
    .fetch_one(&app.db)
    .await?;
    Ok((!off).then_some(merge))
}
/// The lane's merge as of now. The Hub keeps it, so the merged chat's lines reach the lane's
/// sockets; when it changed (or the featured stream `switched`), the feature is remembered as
/// merged (its window's lines stay in the room's history after the switch) and the lane's sockets
/// read the room again. MAGNet's tick and Studio's settings call this, so a co-stream forming or
/// ending, or chat merging turned off, reaches open rooms without each socket polling.
async fn lane_merge(app: &App, lane: &str, switched: bool) -> Res<Option<Merge>> {
    let merge = hype_merge(app, lane).await?;
    let room = format!("magnet:{lane}");
    let now = merge
        .as_ref()
        .map(|m| (m.room().to_string(), m.owners.clone()));
    // Unknown until now (since this process started): no socket here can hold another view.
    let before = app
        .chat
        .lanes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(room.clone(), now.clone());
    let changed = before.as_ref().is_some_and(|before| *before != now);
    if merge.is_some() && (before.is_none() || changed || switched) {
        sqlx::query(
            "UPDATE magnet_features SET merged=true WHERE lane=$1 AND ended_at IS NULL AND NOT merged",
        )
        .bind(lane)
        .execute(&app.db)
        .await?;
    }
    if changed || switched {
        app.chat.publish(&room, None, 0, json!({"type":"magnet"}));
    }
    Ok(merge)
}
/// After MAGNet's tick or a Studio settings change: see `lane_merge`.
pub(crate) async fn follow_lane(app: &App, lane: &str, switched: bool) -> Res<()> {
    // Boxed: callers (the MAGNet tick) are deep already.
    Box::pin(lane_merge(app, lane, switched)).await.map(|_| ())
}
async fn hype_lane(app: &App, lane: &str) -> Res<()> {
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM magnet_lanes WHERE id=$1)")
        .bind(lane)
        .fetch_one(&app.db)
        .await?;
    if found { Ok(()) } else { Err(Fail::missing()) }
}
/// Banned from, timed out in, or blocked by (or blocking) a featured channel, or restricted in the
/// merged co-stream's shared chat: read-only while merged.
async fn hype_holding(app: &App, merge: &Merge, viewer: &str) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM channel_restrictions WHERE channel_id=ANY($1) AND user_id=$2 AND (kind='ban' OR until>now())) OR EXISTS(SELECT 1 FROM user_blocks WHERE (blocker_id=$2 AND blocked_id=ANY($1)) OR (blocker_id=ANY($1) AND blocked_id=$2)) OR EXISTS(SELECT 1 FROM squad_restrictions WHERE squad_id=$3 AND user_id=$2 AND (kind='ban' OR until>now()))")
        .bind(&merge.owners).bind(viewer).bind(&merge.squad).fetch_one(&app.db).await?)
}
/// A MAGNet room as everyone sees it, read once for all its sockets; `hype_snapshot` makes one
/// viewer's copy.
struct HypeRoom {
    merge: Option<Merge>,
    /// Oldest first: each line's author, channel (None in the lane's own room) and message, with
    /// badges and the quote's author kept for each viewer's block filter.
    lines: Vec<(String, Option<String>, Value)>,
    /// Which of these lines' channels and merged owners are labeled mature.
    mature: Vec<String>,
    merged_with: Option<Value>,
    rules: Rules,
}
/// The room's latest 100: its own messages, plus the featured channel's chat since the feature
/// began while merged. Messages from either side stay in both histories after a switch. Shared
/// by everyone asking within a second (a switch makes every socket ask at once) until the room
/// hears anything; with `after`, read after it.
async fn hype_room(app: &App, lane: &str, after: Option<Instant>) -> Res<Arc<HypeRoom>> {
    let room = format!("magnet:{lane}");
    app.chat.hype.get(&room, Duration::from_secs(1), after, || Box::pin(async {
        let merge = lane_merge(app, lane, false).await?;
        // Each branch has its own index: the lane's room, the current merge (a channel or a
        // co-stream's shared chat), and earlier merged windows still within chat's 7-day expiry.
        // select() contains only fixed SQL and a literal chip alias; values are bound.
        let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{} WHERE m.id IN (SELECT id FROM (
                (SELECT m.id,m.seq FROM chat_messages m WHERE m.channel_id IS NULL AND m.origin=$1 AND m.squad_id IS NULL AND {VISIBLE} ORDER BY m.seq DESC LIMIT $4)
                UNION (SELECT m.id,m.seq FROM chat_messages m WHERE $5::text IS NULL AND m.channel_id=$2 AND m.squad_id IS NULL AND m.created_at>=$3 AND {VISIBLE} ORDER BY m.seq DESC LIMIT $4)
                UNION (SELECT m.id,m.seq FROM chat_messages m WHERE m.squad_id=$5 AND m.channel_id=$2 AND m.created_at>=$3 AND {VISIBLE} ORDER BY m.seq DESC LIMIT $4)
                UNION (SELECT w.id,w.seq FROM magnet_features f CROSS JOIN LATERAL (SELECT m.id,m.seq FROM chat_messages m WHERE m.channel_id=f.owner_id AND m.created_at>=f.started_at AND (f.ended_at IS NULL OR m.created_at<f.ended_at) AND {VISIBLE} ORDER BY m.seq DESC LIMIT $4) w
                    WHERE f.lane=$1 AND f.merged AND coalesce(f.ended_at,now())>now()-interval '8 days')
            ) r ORDER BY seq DESC LIMIT $4) ORDER BY m.seq",
            select().replacen("SELECT ", "SELECT m.channel_id,", 1)
        )))
        .bind(lane)
        .bind(merge.as_ref().map(|m| m.channel.as_str()))
        .bind(merge.as_ref().map(|m| m.since))
        .bind(HISTORY)
        .bind(merge.as_ref().and_then(|m| m.squad.as_deref()))
        .fetch_all(&app.db)
        .await?;
        let mut channels: Vec<String> = merge.as_ref().map(|m| m.owners.clone()).unwrap_or_default();
        let mut meta = Vec::with_capacity(rows.len());
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(c) = row.channel_id.as_ref().filter(|c| !channels.contains(c)) {
                channels.push(c.clone());
            }
            meta.push((row.author_id.clone(), row.channel_id.clone()));
            messages.push(row.json(app));
        }
        crate::guilds::hydrate_badges(app, &mut messages).await?;
        let mature: Vec<String> =
            sqlx::query_scalar("SELECT owner_id FROM stream_settings WHERE mature AND owner_id=ANY($1)")
                .bind(&channels)
                .fetch_all(&app.db)
                .await?;
        let (merged_with, rules) = match &merge {
            Some(m) => (
                profiles::channel_user_by_id(&mut *app.db.acquire().await?, &m.channel)
                    .await?
                    .map(|u| profiles::chip(app, &u)),
                hype_rules(app, &m.owners).await?,
            ),
            None => (None, Rules::default()),
        };
        Ok(Arc::new(HypeRoom {
            merge,
            lines: meta
                .into_iter()
                .zip(messages)
                .map(|((author, channel), message)| (author, channel, message))
                .collect(),
            mature,
            merged_with,
            rules,
        }))
    }))
    .await
}
/// One viewer's copy of the room: without people they block (or who block them), without just
/// deleted lines, and closed to under-18 accounts where a channel is labeled mature.
async fn hype_snapshot(
    app: &App,
    lane: &str,
    viewer: Option<&str>,
    blocks: &mut Hidden,
    after: Option<Instant>,
) -> Res<(Value, Option<(String, Vec<String>)>)> {
    let room = hype_room(app, lane, after).await?;
    let merge = room.merge.as_ref();
    // The featured streamers' owners, moderators and staff still see who blocks them.
    blocks.rescope(merge.map(|m| m.owners.clone()).unwrap_or_default());
    let hidden = blocks.current(app, viewer).await?;
    // Chat in a channel labeled mature is closed to under-18 accounts (`mature_gate`), and merging
    // must not open it: its messages stay out of their view of the room, from this merge and
    // earlier ones. A merged co-stream with one labeled member is closed as a whole.
    let mut closed: Vec<&str> = match viewer {
        Some(v)
            if room.mature.iter().any(|o| o != v)
                && !crate::auth::is_adult(&mut *app.db.acquire().await?, v).await? =>
        {
            room.mature
                .iter()
                .map(String::as_str)
                .filter(|o| *o != v)
                .collect()
        }
        _ => Vec::new(),
    };
    let gated = merge.is_some_and(|m| m.owners.iter().any(|o| closed.contains(&o.as_str())));
    if let Some(m) = merge.filter(|_| gated) {
        closed.extend(m.owners.iter().map(String::as_str));
    }
    let messages: Vec<Value> = room
        .lines
        .iter()
        .filter(|(author, channel, message)| {
            !hidden.contains(author)
                && !channel.as_deref().is_some_and(|c| closed.contains(&c))
                && !app.chat.deleted(message["id"].as_str().unwrap_or(""))
        })
        .map(|(_, _, message)| visible_message(message.clone(), hidden))
        .collect();
    // The merged chat's emotes and signature setting; none in MAGNet's own room or while gated.
    let rules = if gated {
        &Rules::default()
    } else {
        &room.rules
    };
    let holding = match (merge, viewer) {
        (Some(m), Some(v)) => gated || hype_holding(app, m, v).await?,
        _ => false,
    };
    // A gated viewer gets no merged room to listen to (`hype_session` then drops live messages).
    Ok((
        json!({"type":"snapshot","messages":messages,"merged_with":room.merged_with,"co_stream":merge.is_some_and(|m| m.squad.is_some()),"holding":holding,"can_send":viewer.is_some() && !holding,"emotes":rules.emotes,"allow_signatures":rules.allow_signatures}),
        merge
            .filter(|_| !gated)
            .map(|m| (m.room().to_string(), m.owners.clone())),
    ))
}
/// A Hype merge's rules: its channel's, or a merged co-stream's members' united (host first).
async fn hype_rules(app: &App, owners: &[String]) -> Res<Rules> {
    match owners {
        [channel] => app.chat.rules(app, channel).await,
        _ => squad_rules(app, &member_names(app, owners).await?).await,
    }
}
/// GET /api/magnet/{lane}/chat
async fn hype_read(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
) -> Res<Json<Value>> {
    hype_lane(&app, &lane).await?;
    let viewer = profiles::viewer(&app, &jar).await?;
    Ok(Json(
        hype_snapshot(
            &app,
            &lane,
            viewer.as_ref().map(|v| v.id.as_str()),
            &mut Hidden::new(Vec::new()),
            None,
        )
        .await?
        .0,
    ))
}
/// POST /api/magnet/{lane}/chat: while merged the message goes into the featured channel's chat
/// under all of its rules (with the MAGNet mark); otherwise it stays in the lane's own room.
async fn hype_post(
    State(app): State<App>,
    jar: CookieJar,
    Path(lane): Path<String>,
    Json(input): Json<Send>,
) -> Res<Json<Value>> {
    hype_lane(&app, &lane).await?;
    let user = profiles::signed_in(&app, &jar).await?;
    let message = match lane_merge(&app, &lane, false).await? {
        Some(m) => {
            if hype_holding(&app, &m, &user.id).await? {
                return Err(Fail::denied("Chat resumes when MAGNet moves on."));
            }
            send_from(
                &app,
                user,
                Some(&m.channel),
                input,
                Some(&lane),
                m.squad.as_deref(),
            )
            .await?
        }
        None => send_from(&app, user, None, input, Some(&lane), None).await?,
    };
    Ok(Json(json!({"message": message})))
}
/// The Hype room over the same-origin WebSocket. Sending goes over HTTPS (same checks).
async fn hype_socket(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(lane): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Res<Response> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(app.config.origin.as_str()) {
        return Err(Fail::denied("Invalid request origin."));
    }
    let ip = network(sec::client_ip(&app, peer, &headers));
    sec::reserve(&app, vec![format!("chat-connect:{ip}")], 30, 60).await?;
    hype_lane(&app, &lane).await?;
    let slot = app.chat.open(ip)?;
    Ok(upgrade
        .max_message_size(FRAME)
        .max_frame_size(FRAME)
        .on_upgrade(move |ws| hype_session(app, jar, lane, slot, ws)))
}
async fn hype_session(
    app: App,
    jar: CookieJar,
    lane: String,
    _slot: SocketSlot,
    mut ws: WebSocket,
) {
    let mut ping = pings();
    let mut alive = Instant::now();
    let room = format!("magnet:{lane}");
    // One room: the Hub passes the merged chat's lines and its owners' emote changes on to it.
    let mut events = app.chat.subscribe_room(&room);
    let subscribed = Instant::now();
    let viewer = profiles::viewer(&app, &jar).await.ok().flatten();
    let viewer = viewer.as_ref().map(|v| v.id.clone());
    let mut blocks = Hidden::new(Vec::new());
    let Ok((first, mut merged)) = hype_snapshot(
        &app,
        &lane,
        viewer.as_deref(),
        &mut blocks,
        Some(subscribed),
    )
    .await
    else {
        return;
    };
    // Holding with no merged room is the mature gate: while it lasts every new message belongs to
    // the closed conversation (MAGNet-side ones arrive on this room too), so none is forwarded.
    let mut gated = first["holding"] == true && merged.is_none();
    if ws
        .send(Message::Text(first.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    // An under-18 viewer's room is re-read every 5 seconds, so a featured channel labeled mature
    // mid-feature (or a mature guest joining its co-stream) stops flowing to them.
    let minor = minor(&app, viewer.as_deref()).await;
    let every = Duration::from_secs(5);
    let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = recheck.tick(), if minor => {
                // Merges and switches arrive as `magnet`; only a mature label can close or open the
                // merged room meanwhile, so the room is read again only when that flips.
                let owners = app.chat.lanes.lock().unwrap_or_else(|e| e.into_inner()).get(&room).cloned().flatten().map(|m| m.1).unwrap_or_default();
                let Ok(mature) = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM stream_settings WHERE mature AND owner_id=ANY($1) AND owner_id IS DISTINCT FROM $2)")
                    .bind(&owners).bind(viewer.as_deref()).fetch_one(&app.db).await else { continue; };
                if mature == gated { continue; }
                let Ok((next, owner)) = hype_snapshot(&app, &lane, viewer.as_deref(), &mut blocks, Some(Instant::now())).await else { continue; };
                let closed = next["holding"] == true && owner.is_none();
                if owner != merged || closed != gated {
                    (merged, gated) = (owner, closed);
                    if ws.send(Message::Text(next.to_string().into())).await.is_err() { return; }
                }
            }
            event = events.recv() => match event {
                Ok(e) => {
                    let kind = e.payload["type"].as_str().unwrap_or_default();
                    if e.channel == room && kind == "magnet" {
                        // Merged, detached or switched: resend the room, read after this event
                        // (every socket here shares that one read).
                        let Ok((next, owner)) = hype_snapshot(&app, &lane, viewer.as_deref(), &mut blocks, Some(e.at)).await else { return; };
                        merged = owner;
                        gated = next["holding"] == true && merged.is_none();
                        if ws.send(Message::Text(next.to_string().into())).await.is_err() { return; }
                        continue;
                    }
                    if e.channel != room {
                        // Passed on from the merged chat (none reach a gated viewer).
                        let Some((merged_room, owners)) = &merged else { continue; };
                        if kind == "emotes" {
                            if !owners.contains(&e.channel) { continue; }
                            let Ok(rules) = hype_rules(&app, owners).await else { continue; };
                            let frame = json!({"type":"emotes","emotes":rules.emotes,"allow_signatures":rules.allow_signatures});
                            if ws.send(Message::Text(frame.to_string().into())).await.is_err() { return; }
                            continue;
                        }
                        // A merged Hype message arrives on both; forward it once.
                        if e.channel != *merged_room || e.payload["message"]["origin"] == lane.as_str() { continue; }
                    }
                    if !matches!(kind, "message" | "delete") { continue; }
                    if gated && kind == "message" { continue; }
                    // Linked-platform lines and the channel bot's stay in the channel's own chat.
                    if !e.payload["message"]["outside"].is_null() { continue; }
                    let Ok(hidden) = blocks.current(&app, viewer.as_deref()).await else { return; };
                    if e.author.as_ref().is_some_and(|a| hidden.contains(a)) { continue; }
                    let mut payload = e.payload.clone();
                    if kind == "message" {
                        let Some(message) = fresh(&app.chat, &payload, hidden) else { continue; };
                        payload["message"] = message;
                    }
                    if ws.send(Message::Text(payload.to_string().into())).await.is_err() { return; }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let _ = ws.send(Message::Close(Some(CloseFrame { code: 4000, reason: "resync".into() }))).await;
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = ping.tick() => {
                // A featured streamer's newly appointed or removed moderator sees the right lines.
                blocks.recheck();
                if alive.elapsed() > SILENT || ws.send(Message::Ping(Default::default())).await.is_err() { return; }
            }
            incoming = ws.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => alive = Instant::now(),
            },
        }
    }
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/channels/{username}/chat", get(read).post(post))
        .route("/api/channels/{username}/chat/pin", put(pin))
        .route("/api/chat/ws", get(socket))
        .route("/api/magnet/{lane}/chat", get(hype_read).post(hype_post))
        .route("/api/magnet/{lane}/ws", get(hype_socket))
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_network_cannot_take_every_chat_socket() {
        let hub = super::Hub::default();
        let a = super::network("2001:db8:1:2::1".parse().unwrap());
        let b = super::network("2001:db8:1:2:ffff::9".parse().unwrap());
        assert_eq!(a, b);
        let held: Vec<_> = (0..super::MAX_PER_ADDRESS)
            .map(|_| hub.open(a).ok().unwrap())
            .collect();
        assert!(hub.open(b).is_err());
        assert!(hub.open("192.0.2.1".parse().unwrap()).is_ok());
        drop(held);
        assert!(hub.open(b).is_ok());
    }
    #[test]
    fn a_message_published_after_its_delete_is_never_delivered() {
        use serde_json::json;
        let hub = super::Hub::default();
        let hidden = std::collections::HashSet::from(["blocked".to_string()]);
        let message = |id: &str| json!({"type":"message","message":{"id":id,"reply":{"id":"orig","author_id":"someone","username":"a","body":"hi"}}});
        // Nothing deleted: delivered, with the quote's internal author ID stripped.
        let shown = super::fresh(&hub, &message("m1"), &hidden).unwrap();
        assert_eq!(shown["reply"]["body"], "hi");
        assert!(shown["reply"].get("author_id").is_none());
        // Its delete raced ahead of it: dropped.
        hub.publish("c", None, 0, json!({"type":"delete","id":"m1"}));
        assert!(super::fresh(&hub, &message("m1"), &hidden).is_none());
        // The quoted message was deleted: the quote is blanked.
        hub.publish("c", None, 0, json!({"type":"delete","id":"orig"}));
        let shown = super::fresh(&hub, &message("m2"), &hidden).unwrap();
        assert!(shown["reply"]["body"].is_null() && shown["reply"]["username"].is_null());
        // A restored message is delivered again.
        hub.restored("m1");
        assert!(super::fresh(&hub, &message("m1"), &hidden).is_some());
    }
    #[test]
    fn a_busy_room_never_lags_another_and_merged_lines_reach_the_lane() {
        use serde_json::json;
        use tokio::sync::broadcast::error::TryRecvError;
        let hub = super::Hub::default();
        let mut quiet = hub.subscribe_room("b");
        let _busy = hub.subscribe_room("a");
        for _ in 0..5_000 {
            hub.publish("a", None, 0, json!({"type":"board_input"}));
        }
        assert!(matches!(quiet.try_recv(), Err(TryRecvError::Empty)));
        hub.publish("b", None, 0, json!({"type":"message","message":{"id":"m"}}));
        assert_eq!(quiet.try_recv().unwrap().payload["message"]["id"], "m");
        // A MAGNet room merged with "b" hears its lines and deletes, not its other events.
        let mut lane = hub.subscribe_room("magnet:l");
        hub.lanes
            .lock()
            .unwrap()
            .insert("magnet:l".into(), Some(("b".into(), vec!["b".into()])));
        hub.publish("b", None, 0, json!({"type":"board_input"}));
        hub.publish("b", None, 0, json!({"type":"delete","id":"m"}));
        let heard = lane.try_recv().unwrap();
        assert_eq!(
            (heard.channel.as_str(), &heard.payload["type"]),
            ("b", &json!("delete"))
        );
        assert!(matches!(lane.try_recv(), Err(TryRecvError::Empty)));
        // Rooms nobody listens to anymore are dropped.
        drop((quiet, lane));
        hub.publish("b", None, 0, json!({"type":"message","message":{"id":"n"}}));
        assert!(!hub.rooms.lock().unwrap().contains_key("b"));
    }
    #[test]
    fn mentions_are_whole_usernames_not_email_or_markup() {
        let mut names = super::mention_names(
            "@Real_User, (@SECOND) @real_user hello@Mailbox.test @@double @ab @abcdefghijklmnopqrstuvwxyz @nameé <img src=x onerror=alert(1)>",
        );
        names.sort();
        assert_eq!(names, ["real_user", "second"]);
    }
}
