//! Developer platform, part 1 (docs/DEVELOPER_PLATFORM.md §1): developer apps, OAuth 2.1 with
//! PKCE (authorization code only; no implicit flow), rotating refresh tokens with reuse detection,
//! revocation, the connected-apps list, and the first scoped API calls. Money is never reachable
//! through an app, and `stream:key` is never granted here.
use crate::{
    App, auth,
    profiles::{self, Fail, Res},
    security as sec,
};
use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const SCOPES: [(&str, &str); 8] = [
    ("user:read", "See your username, display name and faction"),
    ("channel:read", "See your channel's stream settings"),
    ("chat:read", "Read chat as you"),
    ("chat:write", "Send chat messages as you"),
    ("channel:moderate", "Moderate your channel's chat"),
    ("channel:edit", "Change your stream's title and category"),
    (
        "events:private",
        "Receive your channel's private events, such as subscriptions and tributes",
    ),
    (
        "board:control",
        "Control your CrowdSync board like a connected game",
    ),
];
const MAX_APPS: i64 = 10;
const ACCESS_SECONDS: i64 = 3600;
const REFRESH_DAYS: i32 = 60;

/// A redirect URI an app may register: HTTPS, or `http://127.0.0.1` (any port) for desktop apps.
fn check_redirect(uri: &str) -> Option<String> {
    let parsed = url::Url::parse(uri.trim()).ok()?;
    let ok = match parsed.scheme() {
        "https" => parsed.host_str().is_some_and(|h| h != "localhost"),
        "http" => parsed.host_str() == Some("127.0.0.1"),
        _ => false,
    };
    (ok && parsed.fragment().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then(|| parsed.to_string())
}
/// Whether a requested redirect matches a registered one (loopback matches on any port, RFC 8252).
fn redirect_matches(registered: &[String], asked: &str) -> bool {
    let Ok(asked) = url::Url::parse(asked) else {
        return false;
    };
    registered
        .iter()
        .filter_map(|r| url::Url::parse(r).ok())
        .any(|mut r| {
            if r.host_str() == Some("127.0.0.1") && asked.host_str() == Some("127.0.0.1") {
                let _ = r.set_port(asked.port());
            }
            r == asked
        })
}
/// Requested scopes, deduplicated; None when one is unknown (or `stream:key`).
fn parse_scopes(scope: &str) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for s in scope.split_whitespace() {
        if !SCOPES.iter().any(|(k, _)| *k == s) {
            return None;
        }
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    Some(out)
}
fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

// ---- Developer apps (Settings → Developer) ----

async fn developer(app: &App, jar: &CookieJar) -> Res<auth::User> {
    let (tx, user, session) = auth::session(app, jar, false).await?;
    tx.commit().await?;
    auth::authorize_streaming(&user, &session).map_err(|_| {
        Fail::denied("Developer apps need a verified email and two-factor sign-in.")
    })?;
    Ok(user)
}
async fn my_apps(app: &App, owner: &str) -> Res<Json<Value>> {
    let apps: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('client_id',id,'name',name,'redirect_uris',redirect_uris,'confidential',secret_hash IS NOT NULL,'suspended',suspended_at IS NOT NULL,'created_at',created_at,
            'users',(SELECT count(*) FROM oauth_grants g WHERE g.app_id=a.id AND g.revoked_at IS NULL)) FROM dev_apps a WHERE owner_id=$1 ORDER BY created_at")
        .bind(owner).fetch_all(&app.db).await?;
    Ok(Json(json!({"apps": apps, "max": MAX_APPS})))
}
/// GET /api/me/apps
async fn list_apps(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    my_apps(&app, &user.id).await
}
#[derive(Deserialize)]
pub struct NewApp {
    name: String,
    redirect_uris: Vec<String>,
    #[serde(default)]
    confidential: bool,
}
fn clean_app(name: &str, uris: &[String]) -> Res<(String, Vec<String>)> {
    let name = name.trim();
    if !(1..=40).contains(&name.chars().count()) {
        return Err(Fail::field("name", "Name your app (1–40 characters)."));
    }
    if uris.is_empty() || uris.len() > 10 {
        return Err(Fail::field("redirect_uris", "Add 1–10 redirect URIs."));
    }
    let uris: Option<Vec<String>> = uris.iter().map(|u| check_redirect(u)).collect();
    let uris = uris.ok_or_else(|| {
        Fail::field(
            "redirect_uris",
            "Redirect URIs must be https://, or http://127.0.0.1 for desktop apps.",
        )
    })?;
    Ok((name.to_string(), uris))
}
/// POST /api/me/apps: registers an app; a confidential (server) app's secret is shown once.
async fn create_app(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<NewApp>,
) -> Res<Json<Value>> {
    let user = developer(&app, &jar).await?;
    let (name, uris) = clean_app(&input.name, &input.redirect_uris)?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM dev_apps WHERE owner_id=$1")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    if count >= MAX_APPS {
        return Err(Fail::bad("You can register up to 10 apps."));
    }
    let id = format!("sv_{}", &sec::token()[..24]);
    let secret = input.confidential.then(|| format!("svs_{}", sec::token()));
    sqlx::query(
        "INSERT INTO dev_apps(id,owner_id,name,redirect_uris,secret_hash) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(&name)
    .bind(&uris)
    .bind(secret.as_deref().map(sec::digest))
    .execute(&app.db)
    .await?;
    let Json(mut view) = my_apps(&app, &user.id).await?;
    view["created"] = json!({"client_id": id, "client_secret": secret});
    Ok(Json(view))
}
#[derive(Deserialize)]
pub struct ChangeApp {
    name: String,
    redirect_uris: Vec<String>,
}
/// PATCH /api/me/apps/{id}
async fn change_app(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<ChangeApp>,
) -> Res<Json<Value>> {
    let user = developer(&app, &jar).await?;
    let (name, uris) = clean_app(&input.name, &input.redirect_uris)?;
    let changed =
        sqlx::query("UPDATE dev_apps SET name=$3,redirect_uris=$4 WHERE id=$1 AND owner_id=$2")
            .bind(&id)
            .bind(&user.id)
            .bind(&name)
            .bind(&uris)
            .execute(&app.db)
            .await?;
    if changed.rows_affected() == 0 {
        return Err(Fail::missing());
    }
    my_apps(&app, &user.id).await
}
/// POST /api/me/apps/{id}/secret: a new client secret (shown once); the old one stops working.
async fn rotate_secret(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = developer(&app, &jar).await?;
    let secret = format!("svs_{}", sec::token());
    let changed = sqlx::query("UPDATE dev_apps SET secret_hash=$3 WHERE id=$1 AND owner_id=$2")
        .bind(&id)
        .bind(&user.id)
        .bind(sec::digest(&secret))
        .execute(&app.db)
        .await?;
    if changed.rows_affected() == 0 {
        return Err(Fail::missing());
    }
    Ok(Json(json!({"client_id": id, "client_secret": secret})))
}
/// DELETE /api/me/apps/{id}: deletes the app and every grant and token it held.
async fn delete_app(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query("DELETE FROM dev_apps WHERE id=$1 AND owner_id=$2")
        .bind(&id)
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    my_apps(&app, &user.id).await
}

// ---- Consent (the /oauth/authorize page) ----

#[derive(Deserialize, Clone)]
pub struct Authorize {
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    response_type: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: String,
    #[serde(default)]
    code_challenge_method: String,
    #[serde(default)]
    approve: bool,
}
type Request = (String, String, Vec<String>, String);
/// Validates an authorization request: (app name, owner, scopes, redirect).
async fn request(app: &App, q: &Authorize) -> Res<Request> {
    let found: Option<(String, String, Vec<String>)> = sqlx::query_as("SELECT a.name,u.username,a.redirect_uris FROM dev_apps a JOIN users u ON u.id=a.owner_id WHERE a.id=$1 AND a.suspended_at IS NULL")
        .bind(&q.client_id).fetch_optional(&app.db).await?;
    let (name, owner, uris) = found.ok_or_else(|| Fail::bad("That app isn't available."))?;
    if !redirect_matches(&uris, &q.redirect_uri) {
        return Err(Fail::bad("That app's redirect address isn't registered."));
    }
    if q.response_type != "code"
        || q.code_challenge_method != "S256"
        || !(43..=128).contains(&q.code_challenge.len())
    {
        return Err(Fail::bad(
            "The app must use the authorization code flow with PKCE (S256).",
        ));
    }
    let scopes = parse_scopes(&q.scope)
        .ok_or_else(|| Fail::bad("The app asked for a permission S.V.E.R doesn't offer."))?;
    Ok((name, owner, scopes, q.redirect_uri.clone()))
}
/// GET /api/oauth/authorize: what the consent screen shows.
async fn consent(
    State(app): State<App>,
    jar: CookieJar,
    Query(q): Query<Authorize>,
) -> Res<Json<Value>> {
    profiles::signed_in(&app, &jar).await?;
    let (name, owner, scopes, _) = request(&app, &q).await?;
    let described: Vec<Value> = scopes
        .iter()
        .map(|s| {
            let text = SCOPES.iter().find(|(k, _)| k == s).map_or("", |x| x.1);
            json!({"scope": s, "text": text})
        })
        .collect();
    Ok(Json(
        json!({"app": {"name": name, "owner": owner}, "scopes": described}),
    ))
}
/// POST /api/oauth/authorize: the person approves or refuses; returns where to send the browser.
async fn decide(
    State(app): State<App>,
    jar: CookieJar,
    Json(q): Json<Authorize>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let (_, _, scopes, redirect) = request(&app, &q).await?;
    let mut target = url::Url::parse(&redirect).map_err(|_| Fail::bad("Invalid redirect."))?;
    if q.approve {
        let code = sec::token();
        sqlx::query("INSERT INTO oauth_codes(code_hash,app_id,user_id,scopes,redirect_uri,challenge,expires_at) VALUES($1,$2,$3,$4,$5,$6,now()+interval '10 minutes')")
            .bind(sec::digest(&code)).bind(&q.client_id).bind(&user.id).bind(&scopes).bind(&redirect).bind(&q.code_challenge)
            .execute(&app.db).await?;
        target.query_pairs_mut().append_pair("code", &code);
    } else {
        target
            .query_pairs_mut()
            .append_pair("error", "access_denied");
    }
    if let Some(state) = q.state.as_deref() {
        target.query_pairs_mut().append_pair("state", state);
    }
    Ok(Json(json!({"redirect": target.to_string()})))
}

// ---- Token, refresh and revoke (no cookies; client credentials and PKCE) ----

#[derive(Deserialize)]
pub struct TokenRequest {
    grant_type: String,
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    device_code: Option<String>,
}
/// Issues an access and a refresh token under a grant.
async fn issue(app: &App, grant: &str, scopes: &[String]) -> Res<Value> {
    let access = format!("sva_{}", sec::token());
    let refresh = format!("svr_{}", sec::token());
    sqlx::query("INSERT INTO oauth_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,$3,'access',now()+make_interval(secs=>$4)),($2,$3,'refresh',now()+make_interval(days=>$5))")
        .bind(sec::digest(&access)).bind(sec::digest(&refresh)).bind(grant).bind(ACCESS_SECONDS as f64).bind(REFRESH_DAYS)
        .execute(&app.db).await?;
    Ok(
        json!({"access_token": access, "token_type": "Bearer", "expires_in": ACCESS_SECONDS, "refresh_token": refresh, "scope": scopes.join(" ")}),
    )
}
/// POST /api/oauth/token (form-encoded, as OAuth clients send it).
async fn token(State(app): State<App>, Form(input): Form<TokenRequest>) -> Response {
    match exchange(&app, input).await {
        Ok(Ok(body)) => (StatusCode::OK, Json(body)).into_response(),
        Ok(Err((code, why))) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": code, "error_description": why})),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}
type Outcome = Result<Value, (&'static str, &'static str)>;
/// A device poll: (fresh, polled too soon, decision, approver, scopes).
type Polled = (bool, bool, Option<String>, Option<String>, Vec<String>);
type Refresh = (
    String,
    Option<chrono::DateTime<chrono::Utc>>,
    bool,
    bool,
    Vec<String>,
);
async fn exchange(app: &App, input: TokenRequest) -> Res<Outcome> {
    profiles::rate(app, format!("oauth-token:{}", input.client_id), 120, 60).await?;
    let client: Option<(Option<String>, bool)> =
        sqlx::query_as("SELECT secret_hash,suspended_at IS NOT NULL FROM dev_apps WHERE id=$1")
            .bind(&input.client_id)
            .fetch_optional(&app.db)
            .await?;
    let Some((secret_hash, suspended)) = client else {
        return Ok(Err(("invalid_client", "Unknown client.")));
    };
    if suspended {
        return Ok(Err(("invalid_client", "This app is suspended.")));
    }
    if let Some(hash) = secret_hash
        && input.client_secret.as_deref().map(sec::digest).as_deref() != Some(hash.as_str())
    {
        return Ok(Err(("invalid_client", "The client secret is wrong.")));
    }
    match input.grant_type.as_str() {
        "authorization_code" => {
            let code = input.code.unwrap_or_default();
            let found: Option<(String, Vec<String>, String, String, bool)> = sqlx::query_as("DELETE FROM oauth_codes WHERE code_hash=$1 AND app_id=$2 RETURNING user_id,scopes,redirect_uri,challenge,expires_at>now()")
                .bind(sec::digest(&code)).bind(&input.client_id).fetch_optional(&app.db).await?;
            let Some((user, scopes, redirect, challenge, fresh)) = found else {
                return Ok(Err((
                    "invalid_grant",
                    "That code is invalid or was already used.",
                )));
            };
            if !fresh || input.redirect_uri.as_deref() != Some(redirect.as_str()) {
                return Ok(Err((
                    "invalid_grant",
                    "The code expired or the redirect URI doesn't match.",
                )));
            }
            if input.code_verifier.as_deref().map(challenge_of).as_deref()
                != Some(challenge.as_str())
            {
                return Ok(Err((
                    "invalid_grant",
                    "The PKCE code verifier doesn't match.",
                )));
            }
            let grant: String = sqlx::query_scalar("INSERT INTO oauth_grants(id,app_id,user_id,scopes) VALUES($1,$2,$3,$4) ON CONFLICT(app_id,user_id) WHERE revoked_at IS NULL DO UPDATE SET scopes=EXCLUDED.scopes RETURNING id")
                .bind(profiles::new_id()).bind(&input.client_id).bind(&user).bind(&scopes).fetch_one(&app.db).await?;
            Ok(Ok(issue(app, &grant, &scopes).await?))
        }
        "refresh_token" => {
            let token = input.refresh_token.unwrap_or_default();
            let found: Option<Refresh> = sqlx::query_as("SELECT t.grant_id,t.used_at,t.expires_at>now(),g.revoked_at IS NULL,g.scopes FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE t.token_hash=$1 AND t.kind='refresh' AND g.app_id=$2")
                .bind(sec::digest(&token)).bind(&input.client_id).fetch_optional(&app.db).await?;
            let Some((grant, used, fresh, live, scopes)) = found else {
                return Ok(Err(("invalid_grant", "That refresh token is invalid.")));
            };
            if used.is_some() {
                // A spent refresh token came back: someone else may hold it. End the whole grant.
                sqlx::query(
                    "UPDATE oauth_grants SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL",
                )
                .bind(&grant)
                .execute(&app.db)
                .await?;
                return Ok(Err((
                    "invalid_grant",
                    "That refresh token was already used; access was revoked.",
                )));
            }
            if !fresh || !live {
                return Ok(Err((
                    "invalid_grant",
                    "That refresh token expired or was revoked.",
                )));
            }
            let spent = sqlx::query(
                "UPDATE oauth_tokens SET used_at=now() WHERE token_hash=$1 AND used_at IS NULL",
            )
            .bind(sec::digest(&token))
            .execute(&app.db)
            .await?;
            if spent.rows_affected() == 0 {
                return Ok(Err((
                    "invalid_grant",
                    "That refresh token was already used.",
                )));
            }
            Ok(Ok(issue(app, &grant, &scopes).await?))
        }
        "urn:ietf:params:oauth:grant-type:device_code" => {
            let device = sec::digest(input.device_code.as_deref().unwrap_or_default());
            let found: Option<Polled> = sqlx::query_as("UPDATE oauth_devices d SET last_poll_at=now() FROM (SELECT device_hash,last_poll_at AS previous FROM oauth_devices WHERE device_hash=$1 AND app_id=$2) p
                WHERE d.device_hash=p.device_hash RETURNING d.expires_at>now(),coalesce(p.previous>now()-interval '5 seconds',false),d.decision,d.user_id,d.scopes")
                .bind(&device).bind(&input.client_id).fetch_optional(&app.db).await?;
            let Some((fresh, too_soon, decision, user, scopes)) = found else {
                return Ok(Err(("invalid_grant", "Unknown device code.")));
            };
            if !fresh {
                return Ok(Err(("expired_token", "The code expired; start again.")));
            }
            match (decision.as_deref(), user) {
                (Some("approved"), Some(user)) => {
                    // Used once: the device code is gone after the tokens are issued.
                    let taken = sqlx::query("DELETE FROM oauth_devices WHERE device_hash=$1")
                        .bind(&device)
                        .execute(&app.db)
                        .await?;
                    if taken.rows_affected() == 0 {
                        return Ok(Err(("invalid_grant", "That code was already used.")));
                    }
                    let grant: String = sqlx::query_scalar("INSERT INTO oauth_grants(id,app_id,user_id,scopes) VALUES($1,$2,$3,$4) ON CONFLICT(app_id,user_id) WHERE revoked_at IS NULL DO UPDATE SET scopes=EXCLUDED.scopes RETURNING id")
                        .bind(profiles::new_id()).bind(&input.client_id).bind(&user).bind(&scopes).fetch_one(&app.db).await?;
                    Ok(Ok(issue(app, &grant, &scopes).await?))
                }
                (Some(_), _) => {
                    sqlx::query("DELETE FROM oauth_devices WHERE device_hash=$1")
                        .bind(&device)
                        .execute(&app.db)
                        .await?;
                    Ok(Err(("access_denied", "The person refused.")))
                }
                _ if too_soon => Ok(Err(("slow_down", "Poll at most every 5 seconds."))),
                _ => Ok(Err((
                    "authorization_pending",
                    "Waiting for approval at sver.tv/go.",
                ))),
            }
        }
        _ => Ok(Err((
            "unsupported_grant_type",
            "Use authorization_code or refresh_token.",
        ))),
    }
}
#[derive(Deserialize)]
pub struct RevokeRequest {
    token: String,
    client_id: String,
}
/// POST /api/oauth/revoke (RFC 7009): ends the grant the token belongs to. Always 200.
async fn revoke(State(app): State<App>, Form(input): Form<RevokeRequest>) -> Res<Json<Value>> {
    sqlx::query("UPDATE oauth_grants g SET revoked_at=now() FROM oauth_tokens t WHERE t.token_hash=$1 AND t.grant_id=g.id AND g.app_id=$2 AND g.revoked_at IS NULL")
        .bind(sec::digest(&input.token)).bind(&input.client_id).execute(&app.db).await?;
    Ok(Json(json!({})))
}

// ---- Device sign-in at sver.tv/go (RFC 8628) ----

/// No vowels (no words) and no lookalikes (0/O, 1/I/L).
const CODE_ALPHABET: &[u8] = b"BCDFGHJKMNPQRSTVWXZ23456789";
fn user_code() -> String {
    let mut bits = uuid::Uuid::new_v4().as_u128();
    (0..6)
        .map(|_| {
            let c = CODE_ALPHABET[(bits % CODE_ALPHABET.len() as u128) as usize] as char;
            bits /= CODE_ALPHABET.len() as u128;
            c
        })
        .collect()
}
/// "abc-def", "ABC DEF" and "ABCDEF" are the same code.
fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}
#[derive(Deserialize)]
pub struct DeviceRequest {
    client_id: String,
    #[serde(default)]
    scope: String,
}
/// POST /api/oauth/device: a desktop, TV or console app asks for a code to show.
async fn device(
    State(app): State<App>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Form(input): Form<DeviceRequest>,
) -> Response {
    let ip = sec::client_ip(&app, peer, &headers);
    if let Err(error) = profiles::rate(&app, format!("oauth-device:{ip}"), 20, 3600).await {
        return error.into_response();
    }
    let known: Result<bool, sqlx::Error> = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM dev_apps WHERE id=$1 AND suspended_at IS NULL)",
    )
    .bind(&input.client_id)
    .fetch_one(&app.db)
    .await;
    if !known.unwrap_or(false) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_client"})),
        )
            .into_response();
    }
    let Some(scopes) = parse_scopes(&input.scope) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_scope"})),
        )
            .into_response();
    };
    let device = format!("svd_{}", sec::token());
    let code = user_code();
    let network = app.config.networks.describe(ip);
    let saved = sqlx::query("INSERT INTO oauth_devices(device_hash,user_code,app_id,scopes,network,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '10 minutes')")
        .bind(sec::digest(&device)).bind(&code).bind(&input.client_id).bind(&scopes).bind(&network)
        .execute(&app.db).await;
    if saved.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "temporarily_unavailable"})),
        )
            .into_response();
    }
    let shown = format!("{}-{}", &code[..3], &code[3..]);
    let page = format!("{}/go", app.config.origin);
    Json(json!({"device_code": device, "user_code": shown, "verification_uri": page, "verification_uri_complete": format!("{page}?code={shown}"), "expires_in": 600, "interval": 5})).into_response()
}
type Pending = (String, String, Vec<String>, Option<String>);
async fn pending(app: &App, code: &str) -> Res<Pending> {
    let found: Option<Pending> = sqlx::query_as("SELECT a.name,u.username,d.scopes,d.network FROM oauth_devices d JOIN dev_apps a ON a.id=d.app_id AND a.suspended_at IS NULL JOIN users u ON u.id=a.owner_id
        WHERE d.user_code=$1 AND d.expires_at>now() AND d.decision IS NULL")
        .bind(normalize(code)).fetch_optional(&app.db).await?;
    found.ok_or_else(|| {
        Fail::bad("That code is wrong or has expired. Check the code on your device.")
    })
}
/// GET /api/oauth/device/{code}: what /go shows before the person approves.
async fn device_lookup(
    State(app): State<App>,
    jar: CookieJar,
    Path(code): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("oauth-go:{}", user.id), 20, 600).await?;
    let (name, owner, scopes, network) = pending(&app, &code).await?;
    let described: Vec<Value> = scopes
        .iter()
        .map(|s| json!({"scope": s, "text": SCOPES.iter().find(|(k, _)| k == s).map_or("", |x| x.1)}))
        .collect();
    Ok(Json(
        json!({"app": {"name": name, "owner": owner}, "scopes": described, "network": network}),
    ))
}
#[derive(Deserialize)]
pub struct DeviceDecision {
    approve: bool,
}
/// POST /api/oauth/device/{code}: approve or refuse; the device then gets its tokens (or not).
async fn device_decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(code): Path<String>,
    Json(input): Json<DeviceDecision>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    profiles::rate(&app, format!("oauth-go:{}", user.id), 20, 600).await?;
    pending(&app, &code).await?;
    sqlx::query("UPDATE oauth_devices SET user_id=$2,decision=$3 WHERE user_code=$1 AND decision IS NULL AND expires_at>now()")
        .bind(normalize(&code)).bind(&user.id).bind(if input.approve { "approved" } else { "denied" })
        .execute(&app.db).await?;
    Ok(Json(json!({"approved": input.approve})))
}

// ---- Bearer access for the API ----

fn unauthorized(message: &'static str) -> Fail {
    Fail::new(StatusCode::UNAUTHORIZED, message)
}
/// The calling app from `SVER-Client-Id` (required on every API call), and the person when a
/// bearer token is given; `scope` is required when set. Rate-limited per app and person.
pub async fn caller(
    app: &App,
    headers: &HeaderMap,
    scope: Option<&str>,
) -> Res<(String, Option<String>)> {
    let client = headers
        .get("sver-client-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| unauthorized("Send your app's SVER-Client-Id header."))?
        .to_string();
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let user = match token {
        Some(token) => {
            let found: Option<(String, String, Vec<String>)> = sqlx::query_as("UPDATE oauth_grants g SET last_used_at=now() FROM oauth_tokens t, dev_apps a
                WHERE t.token_hash=$1 AND t.kind='access' AND t.expires_at>now() AND g.id=t.grant_id AND g.revoked_at IS NULL AND a.id=g.app_id AND a.suspended_at IS NULL
                RETURNING g.user_id,g.app_id,g.scopes")
                .bind(sec::digest(token)).fetch_optional(&app.db).await?;
            let (user, app_id, scopes) =
                found.ok_or_else(|| unauthorized("That access token is invalid or expired."))?;
            if app_id != client {
                return Err(unauthorized("That token belongs to another app."));
            }
            if scope.is_some_and(|s| !scopes.iter().any(|g| g == s)) {
                return Err(Fail::denied(
                    "This token doesn't have the permission for that.",
                ));
            }
            Some(user)
        }
        None if scope.is_some() => return Err(unauthorized("This call needs an access token.")),
        None => {
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM dev_apps WHERE id=$1 AND suspended_at IS NULL)",
            )
            .bind(&client)
            .fetch_one(&app.db)
            .await?;
            if !known {
                return Err(unauthorized("Unknown or suspended app."));
            }
            None
        }
    };
    let who = user.as_deref().unwrap_or("-");
    profiles::rate(app, format!("api:{client}:{who}"), 600, 60).await?;
    Ok((client, user))
}
/// GET /api/v1/me (user:read)
async fn v1_me(State(app): State<App>, headers: HeaderMap) -> Res<Json<Value>> {
    let (_, user) = caller(&app, &headers, Some("user:read")).await?;
    let user = user.ok_or_else(Fail::missing)?;
    let me: Value = sqlx::query_scalar("SELECT jsonb_build_object('username',c.username,'display_name',c.display_name,'faction',(SELECT faction FROM faction_members WHERE user_id=c.id)) FROM channel_users c WHERE c.id=$1")
        .bind(&user).fetch_one(&app.db).await?;
    Ok(Json(me))
}
/// GET /api/v1/channels/{name}: a channel's public information (client ID only).
async fn v1_channel(
    State(app): State<App>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Res<Json<Value>> {
    caller(&app, &headers, None).await?;
    let channel: Option<Value> = sqlx::query_scalar("SELECT jsonb_build_object('username',c.username,'display_name',c.display_name,'faction',(SELECT faction FROM faction_members WHERE user_id=c.id),
            'live',EXISTS(SELECT 1 FROM broadcasts b WHERE b.owner_id=c.id AND b.state IN ('LIVE','RECONNECTING')),
            'title',s.title,'category',k.name,'followers',(SELECT count(*) FROM follows WHERE following_id=c.id))
        FROM channel_users c LEFT JOIN stream_settings s ON s.owner_id=c.id LEFT JOIN stream_categories k ON k.id=s.category_id
        WHERE lower(c.username)=lower($1) AND c.eligible")
        .bind(&name).fetch_optional(&app.db).await?;
    Ok(Json(channel.ok_or_else(Fail::missing)?))
}

// ---- Connected apps (Settings → Connections) and staff ----

/// GET /api/me/connections
async fn connections(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',g.id,'app',a.name,'owner',u.username,'scopes',g.scopes,'created_at',g.created_at,'last_used_at',g.last_used_at)
        FROM oauth_grants g JOIN dev_apps a ON a.id=g.app_id JOIN users u ON u.id=a.owner_id WHERE g.user_id=$1 AND g.revoked_at IS NULL ORDER BY g.created_at DESC")
        .bind(&user.id).fetch_all(&app.db).await?;
    Ok(Json(json!({"connections": rows})))
}
/// DELETE /api/me/connections/{id}: revokes an app's access; its tokens stop working at once.
async fn disconnect(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    sqlx::query(
        "UPDATE oauth_grants SET revoked_at=now() WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL",
    )
    .bind(&id)
    .bind(&user.id)
    .execute(&app.db)
    .await?;
    connections(State(app), jar).await
}
/// GET /api/admin/apps: every app, for staff.
async fn staff_apps(State(app): State<App>, jar: CookieJar) -> Res<Json<Value>> {
    crate::safety::staff(&app, &jar).await?;
    let rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('client_id',a.id,'name',a.name,'owner',u.username,'redirect_uris',a.redirect_uris,'suspended',a.suspended_at IS NOT NULL,'created_at',a.created_at,
            'users',(SELECT count(*) FROM oauth_grants g WHERE g.app_id=a.id AND g.revoked_at IS NULL))
        FROM dev_apps a JOIN users u ON u.id=a.owner_id ORDER BY a.created_at DESC LIMIT 500")
        .fetch_all(&app.db).await?;
    Ok(Json(json!({"apps": rows})))
}
#[derive(Deserialize)]
pub struct Suspend {
    suspended: bool,
}
/// POST /api/admin/apps/{id}/suspend: a suspended app's tokens and logins stop working.
async fn suspend(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Suspend>,
) -> Res<Json<Value>> {
    let staff = crate::safety::staff(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE dev_apps SET suspended_at=CASE WHEN $2 THEN coalesce(suspended_at,now()) END WHERE id=$1")
        .bind(&id).bind(input.suspended).execute(&mut *tx).await?;
    let action = if input.suspended {
        "app_suspend"
    } else {
        "app_restore"
    };
    crate::safety::audit(
        &mut tx,
        Some(&staff.id),
        action,
        "dev_app",
        &id,
        &[],
        "Developer app",
        json!({}),
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"suspended": input.suspended})))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/me/apps", get(list_apps).post(create_app))
        .route(
            "/api/me/apps/{id}",
            axum::routing::patch(change_app).delete(delete_app),
        )
        .route("/api/me/apps/{id}/secret", post(rotate_secret))
        .route("/api/oauth/authorize", get(consent).post(decide))
        .route("/api/oauth/token", post(token))
        .route("/api/oauth/revoke", post(revoke))
        .route("/api/oauth/device", post(device))
        .route(
            "/api/oauth/device/{code}",
            get(device_lookup).post(device_decide),
        )
        .route("/api/v1/me", get(v1_me))
        .route("/api/v1/channels/{name}", get(v1_channel))
        .route("/api/me/connections", get(connections))
        .route("/api/me/connections/{id}", delete(disconnect))
        .route("/api/admin/apps", get(staff_apps))
        .route("/api/admin/apps/{id}/suspend", post(suspend))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirects_scopes_and_pkce() {
        assert!(check_redirect("https://example.com/cb").is_some());
        assert!(check_redirect("http://127.0.0.1:4000/cb").is_some());
        assert!(check_redirect("http://example.com/cb").is_none());
        assert!(check_redirect("https://localhost/cb").is_none());
        assert!(check_redirect("https://example.com/cb#x").is_none());
        let registered = vec![
            "http://127.0.0.1/cb".to_string(),
            "https://example.com/cb".to_string(),
        ];
        assert!(redirect_matches(&registered, "http://127.0.0.1:51234/cb"));
        assert!(!redirect_matches(&registered, "https://example.com/other"));
        assert_eq!(
            parse_scopes("user:read chat:read user:read"),
            Some(vec!["user:read".into(), "chat:read".into()])
        );
        assert_eq!(parse_scopes("stream:key"), None);
        let code = user_code();
        assert!(code.len() == 6 && code.bytes().all(|b| CODE_ALPHABET.contains(&b)));
        assert_eq!(normalize("bcd-fg h"), "BCDFGH");
        // RFC 7636 appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
