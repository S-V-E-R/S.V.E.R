use crate::{App, Error, Result, auth, security as sec};
use axum::{
    Json,
    extract::{ConnectInfo, Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;

#[derive(Clone)]
pub struct Provider {
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorize_url: String,
    pub token_url: String,
    pub profile_url: String,
    pub scopes: String,
    pub pkce: bool,
    pub redirect_uri: Option<String>,
}
pub fn providers_from_env() -> std::result::Result<Vec<Provider>, String> {
    let mut providers = Vec::new();
    for (name, authorize, token, profile, scopes, pkce) in [
        (
            "google",
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
            "https://openidconnect.googleapis.com/v1/userinfo",
            "openid email profile",
            true,
        ),
        (
            "twitch",
            "https://id.twitch.tv/oauth2/authorize",
            "https://id.twitch.tv/oauth2/token",
            "https://api.twitch.tv/helix/users",
            "user:read:email",
            false,
        ),
        (
            "discord",
            "https://discord.com/oauth2/authorize",
            "https://discord.com/api/oauth2/token",
            "https://discord.com/api/users/@me",
            "identify email",
            true,
        ),
    ] {
        let client_id =
            std::env::var(format!("{}_CLIENT_ID", name.to_uppercase())).unwrap_or_default();
        let client_secret =
            std::env::var(format!("{}_CLIENT_SECRET", name.to_uppercase())).unwrap_or_default();
        if client_id.is_empty() && client_secret.is_empty() {
            continue;
        }
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(format!("Both {name} OAuth credentials are required"));
        }
        let redirect_uri = std::env::var(format!("{}_REDIRECT_URI", name.to_uppercase()))
            .ok()
            .filter(|value| !value.is_empty());
        if let Some(value) = &redirect_uri {
            let uri = url::Url::parse(value).map_err(|_| format!("Invalid {name} redirect URI"))?;
            if !(uri.scheme() == "https"
                || (uri.scheme() == "http"
                    && matches!(uri.host_str(), Some("localhost" | "127.0.0.1"))))
                || uri.host_str().is_none()
                || !uri.username().is_empty()
                || uri.password().is_some()
                || uri.query().is_some()
                || uri.fragment().is_some()
                || uri.path() != format!("/api/auth/oauth/{name}/callback")
            {
                return Err(format!(
                    "{name} redirect URI must be an HTTPS callback URL (loopback HTTP allowed for development)"
                ));
            }
        }
        providers.push(Provider {
            name: name.into(),
            client_id,
            client_secret,
            authorize_url: authorize.into(),
            token_url: token.into(),
            profile_url: profile.into(),
            scopes: scopes.into(),
            pkce,
            redirect_uri,
        });
    }
    Ok(providers)
}
fn provider<'a>(app: &'a App, name: &str) -> Result<&'a Provider> {
    app.config
        .providers
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| Error::bad("That sign-in provider is not configured."))
}
fn callback_uri(app: &App, p: &Provider) -> String {
    p.redirect_uri
        .clone()
        .unwrap_or_else(|| format!("{}/api/auth/oauth/{}/callback", app.config.origin, p.name))
}
#[derive(Deserialize)]
pub struct Start {
    intent: String,
    #[serde(default)]
    code: String,
}
#[derive(Serialize, Deserialize)]
struct Payload {
    verifier: String,
}
pub async fn start(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(input): Json<Start>,
) -> Result<(CookieJar, Json<Value>)> {
    let p = provider(&app, &name)?;
    if !["login", "signup", "link", "reauth"].contains(&input.intent.as_str()) {
        return Err(Error::bad("Invalid OAuth action."));
    }
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("oauth:start:{ip}")], 20, 900).await?;
    let state = sec::token();
    let browser = sec::token();
    let verifier = sec::token();
    let payload = sec::seal(
        &app,
        "oauth",
        &serde_json::to_string(&Payload {
            verifier: verifier.clone(),
        })
        .map_err(|_| Error::internal())?,
    )?;
    let (mut tx, session_id) = if input.intent == "link" {
        let (tx, _, session) = auth::sensitive(&app, &jar, &input.code).await?;
        (tx, Some(session.id))
    } else if input.intent == "reauth" {
        let (tx, _, session) = auth::session(&app, &jar, true).await?;
        (tx, Some(session.id))
    } else {
        (app.db.begin().await?, None)
    };
    sqlx::query("INSERT INTO oauth_states(state_hash,browser_hash,provider,intent,session_id,payload) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(sec::digest(&state)).bind(sec::digest(&browser)).bind(&name).bind(&input.intent).bind(session_id).bind(payload).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut url = url::Url::parse(&p.authorize_url).map_err(|_| Error::internal())?;
    url.query_pairs_mut()
        .append_pair("client_id", &p.client_id)
        .append_pair("redirect_uri", &callback_uri(&app, p))
        .append_pair("response_type", "code")
        .append_pair("scope", &p.scopes)
        .append_pair("state", &state);
    if p.pkce {
        url.query_pairs_mut()
            .append_pair("code_challenge_method", "S256")
            .append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            );
    }
    if input.intent == "reauth" || input.intent == "link" {
        if p.name == "twitch" {
            url.query_pairs_mut().append_pair("force_verify", "true");
        } else {
            url.query_pairs_mut().append_pair(
                "prompt",
                if p.name == "google" {
                    "select_account"
                } else {
                    "consent"
                },
            );
        }
    }
    Ok((
        jar.add(auth::cookie(
            &app,
            &auth::aux_name(&app, "oauth"),
            &browser,
            600,
        )),
        Json(json!({"url":url.to_string()})),
    ))
}
#[derive(Deserialize)]
pub struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}
#[derive(sqlx::FromRow)]
struct OAuthState {
    intent: String,
    session_id: Option<String>,
    payload: String,
}
struct Identity {
    subject: String,
    email: Option<String>,
    verified: bool,
    username: Option<String>,
}
async fn exchange(app: &App, p: &Provider, code: &str, verifier: &str) -> Result<Identity> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", p.client_id.as_str()),
        ("client_secret", p.client_secret.as_str()),
    ];
    let redirect = callback_uri(app, p);
    form.push(("redirect_uri", &redirect));
    if p.pkce {
        form.push(("code_verifier", verifier));
    }
    let token: Value = app
        .http
        .post(&p.token_url)
        .form(&form)
        .send()
        .await
        .map_err(|_| Error::unavailable())?
        .error_for_status()
        .map_err(|_| Error::bad("Provider authorization failed. Start again."))?
        .json()
        .await
        .map_err(|_| Error::unavailable())?;
    let access = token["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::bad("Provider did not return an access token."))?;
    if !token["token_type"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::bad("Unsupported provider token."));
    }
    let mut request = app.http.get(&p.profile_url).bearer_auth(access);
    if p.name == "twitch" {
        request = request.header("Client-Id", &p.client_id);
    }
    let body: Value = request
        .send()
        .await
        .map_err(|_| Error::unavailable())?
        .error_for_status()
        .map_err(|_| Error::bad("Could not verify the provider identity."))?
        .json()
        .await
        .map_err(|_| Error::unavailable())?;
    let body = if p.name == "twitch" {
        body["data"]
            .as_array()
            .filter(|a| a.len() == 1)
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| Error::bad("Provider did not return one identity."))?
    } else {
        body
    };
    let subject = body[if p.name == "google" { "sub" } else { "id" }]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 255)
        .ok_or_else(|| Error::bad("Provider identity is missing."))?
        .to_owned();
    let email = body["email"].as_str().map(sec::email).transpose()?;
    // Twitch Helix does not expose an email-verification flag. Send our own verification link.
    let verified = match p.name.as_str() {
        "google" => body["email_verified"] == true,
        "discord" => body["verified"] == true,
        _ => false,
    };
    Ok(Identity {
        subject,
        email,
        verified,
        username: body[if p.name == "twitch" {
            "login"
        } else if p.name == "discord" {
            "username"
        } else {
            "name"
        }]
        .as_str()
        .map(str::to_owned),
    })
}
pub async fn callback(
    State(app): State<App>,
    headers: HeaderMap,
    jar: CookieJar,
    Path(name): Path<String>,
    Query(query): Query<Callback>,
) -> Response {
    let result = complete(&app, &headers, jar.clone(), &name, query).await;
    match result {
        Ok((jar, target)) => (jar, Redirect::to(&target)).into_response(),
        Err(error) => {
            let provider = match name.as_str() {
                "google" | "twitch" | "discord" => name.as_str(),
                _ => "unknown",
            };
            eprintln!(
                "auth_event=oauth_rejected provider={provider} status={}",
                error.0.as_u16()
            );
            // Fixed local redirect; no provider error strings or authorization codes are reflected.
            let mut url = url::Url::parse(&format!("{}/login", app.config.origin)).unwrap();
            url.query_pairs_mut().append_pair("error", error.1);
            (
                auth::clear_cookie(&app, jar, &auth::aux_name(&app, "oauth")),
                Redirect::to(url.as_str()),
            )
                .into_response()
        }
    }
}
async fn complete(
    app: &App,
    headers: &HeaderMap,
    jar: CookieJar,
    name: &str,
    query: Callback,
) -> Result<(CookieJar, String)> {
    let p = provider(app, name)?;
    if query.state.len() > 128 {
        return Err(Error::bad("Invalid OAuth state."));
    }
    let browser = jar
        .get(&auth::aux_name(app, "oauth"))
        .ok_or_else(|| Error::bad("OAuth browser session expired. Start again."))?
        .value();
    let state:OAuthState=sqlx::query_as("DELETE FROM oauth_states WHERE state_hash=$1 AND browser_hash=$2 AND provider=$3 AND expires_at>now() RETURNING *")
        .bind(sec::digest(&query.state)).bind(sec::digest(browser)).bind(name).fetch_optional(&app.db).await?.ok_or_else(||Error::bad("OAuth state expired or was already used."))?;
    let jar = auth::clear_cookie(app, jar, &auth::aux_name(app, "oauth"));
    if query.error.is_some() {
        return Err(Error::bad("Provider sign-in was cancelled."));
    }
    let code = query
        .code
        .filter(|c| !c.is_empty() && c.len() <= 4096)
        .ok_or_else(|| Error::bad("Provider authorization code is missing."))?;
    let payload: Payload = serde_json::from_str(&sec::unseal(app, "oauth", &state.payload)?)
        .map_err(|_| Error::internal())?;
    let identity = exchange(app, p, &code, &payload.verifier).await?;
    if state.intent == "link" || state.intent == "reauth" {
        let (mut tx, user, session) = auth::session(app, &jar, state.intent == "reauth").await?;
        if state.session_id.as_deref() != Some(&session.id) {
            return Err(Error::auth());
        }
        if state.intent == "link" {
            auth::recent(&session)?;
            sqlx::query("INSERT INTO identities(provider,subject,user_id) VALUES($1,$2,$3)")
                .bind(name)
                .bind(identity.subject)
                .bind(user.id)
                .execute(&mut *tx)
                .await?;
        } else {
            let linked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identities WHERE provider=$1 AND subject=$2 AND user_id=$3)").bind(name).bind(identity.subject).bind(user.id).fetch_one(&mut *tx).await?;
            if !linked {
                return Err(Error::denied(
                    "Reauthenticate with a provider already linked to this account.",
                ));
            }
            sqlx::query("UPDATE sessions SET authenticated_at=now() WHERE id=$1")
                .bind(session.id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        eprintln!(
            "auth_event=oauth_verified provider={} intent={} next=account",
            p.name, state.intent
        );
        return Ok((
            auth::renew(app, jar),
            format!("{}/account", app.config.origin),
        ));
    }
    let existing: Option<String> =
        sqlx::query_scalar("SELECT user_id FROM identities WHERE provider=$1 AND subject=$2")
            .bind(name)
            .bind(&identity.subject)
            .fetch_optional(&app.db)
            .await?;
    let mut tx = app.db.begin().await?;
    let user = if let Some(id) = existing {
        let user = auth::lock_user(&mut tx, &id).await?;
        let still_linked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identities WHERE provider=$1 AND subject=$2 AND user_id=$3)").bind(name).bind(&identity.subject).bind(&id).fetch_one(&mut *tx).await?;
        if !still_linked {
            return Err(Error::auth());
        }
        user
    } else {
        let email = identity.email.ok_or_else(|| {
            Error::bad("Allow email access at the provider, or sign up with email.")
        })?;
        if name != "twitch" && !identity.verified {
            return Err(Error::bad(
                "Verify your email with the provider first, or sign up with email.",
            ));
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE lower(email)=$1)")
                .bind(&email)
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            return Err(Error::bad(
                "Sign in to your existing S.V.E.R account, then link this provider in settings.",
            ));
        }
        let mut username = format!("{name}_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        for candidate in [identity.username.as_deref(), email.split('@').next()]
            .into_iter()
            .flatten()
        {
            let candidate: String = candidate
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(25)
                .collect();
            if sec::validate_username(&candidate).is_ok() {
                let taken: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE lower(username)=lower($1)) OR EXISTS(SELECT 1 FROM username_holds WHERE handle_canonical=lower($1) AND released_at>now())",
                )
                .bind(&candidate)
                .fetch_one(&mut *tx)
                .await?;
                if !taken {
                    username = candidate;
                    break;
                }
            }
        }
        let pending = PendingIdentity {
            subject: identity.subject,
            email,
            verified: identity.verified,
            username,
        };
        let token = sec::token();
        if let Some(previous) = jar.get(&auth::aux_name(app, "signup")) {
            sqlx::query("DELETE FROM oauth_signups WHERE token_hash=$1")
                .bind(sec::digest(previous.value()))
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("INSERT INTO oauth_signups(token_hash,provider,payload) VALUES($1,$2,$3)")
            .bind(sec::digest(&token))
            .bind(name)
            .bind(sec::seal(
                app,
                "oauth-signup",
                &serde_json::to_string(&pending).map_err(|_| Error::internal())?,
            )?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        eprintln!(
            "auth_event=oauth_verified provider={} intent={} next=signup",
            p.name, state.intent
        );
        let jar = auth::clear_cookie(app, jar, app.config.cookie_name()).add(auth::cookie(
            app,
            &auth::aux_name(app, "signup"),
            &token,
            600,
        ));
        return Ok((jar, format!("{}/oauth-signup", app.config.origin)));
    };
    let (jar, result) = auth::finish_login(app, &mut tx, &user, jar, headers, None).await?;
    tx.commit().await?;
    // Only reached after a real provider token exchange, identity lookup and committed local checks.
    // No account identifiers, authorization codes, tokens, cookies or provider response bodies are logged.
    eprintln!(
        "auth_event=oauth_verified provider={} intent={} next={}",
        p.name,
        state.intent,
        if result["requires_mfa"] == true {
            "mfa"
        } else {
            "account"
        }
    );
    Ok((
        jar,
        format!(
            "{}/{}",
            app.config.origin,
            if result["requires_mfa"] == true {
                "mfa"
            } else {
                "account"
            }
        ),
    ))
}
#[derive(Serialize, Deserialize)]
struct PendingIdentity {
    subject: String,
    email: String,
    verified: bool,
    username: String,
}
#[derive(sqlx::FromRow)]
struct PendingSignup {
    provider: String,
    payload: String,
}
fn signup_token(app: &App, jar: &CookieJar) -> Result<String> {
    jar.get(&auth::aux_name(app, "signup"))
        .map(|cookie| sec::digest(cookie.value()))
        .ok_or_else(|| Error::bad("Your signup session expired. Start again with your provider."))
}
pub async fn signup_details(State(app): State<App>, jar: CookieJar) -> Result<Json<Value>> {
    let pending: PendingSignup = sqlx::query_as(
        "SELECT provider,payload FROM oauth_signups WHERE token_hash=$1 AND expires_at>now()",
    )
    .bind(signup_token(&app, &jar)?)
    .fetch_optional(&app.db)
    .await?
    .ok_or_else(|| Error::bad("Your signup session expired. Start again with your provider."))?;
    let identity: PendingIdentity =
        serde_json::from_str(&sec::unseal(&app, "oauth-signup", &pending.payload)?)
            .map_err(|_| Error::internal())?;
    Ok(Json(
        json!({"provider":pending.provider,"username":identity.username}),
    ))
}
#[derive(Deserialize)]
pub struct FinishSignup {
    username: String,
    date_of_birth: String,
    turnstile_token: String,
}
pub async fn finish_signup(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(input): Json<FinishSignup>,
) -> Result<(CookieJar, Json<Value>)> {
    let dob = sec::signup_identity(&input.username, &input.date_of_birth)?;
    let token_hash = signup_token(&app, &jar)?;
    let ip = sec::client_ip(&app, peer, &headers);
    sec::reserve(&app, vec![format!("signup:{ip}")], 5, 900).await?;
    sec::turnstile(&app, &input.turnstile_token, "signup", ip).await?;
    let mut tx = app.db.begin().await?;
    let pending: PendingSignup = sqlx::query_as("DELETE FROM oauth_signups WHERE token_hash=$1 AND expires_at>now() RETURNING provider,payload")
        .bind(token_hash).fetch_optional(&mut *tx).await?
        .ok_or_else(|| Error::bad("Your signup session expired. Start again with your provider."))?;
    let identity: PendingIdentity =
        serde_json::from_str(&sec::unseal(&app, "oauth-signup", &pending.payload)?)
            .map_err(|_| Error::internal())?;
    // A conflicting email/subject is rejected by unique constraints; never auto-link by email.
    let user = auth::create_user(
        &mut tx,
        &identity.email,
        &input.username,
        dob,
        None,
        identity.verified,
    )
    .await?;
    sqlx::query("INSERT INTO identities(provider,subject,user_id) VALUES($1,$2,$3)")
        .bind(&pending.provider)
        .bind(identity.subject)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    if !user.email_verified {
        crate::jobs::queue_email(&app, &mut tx, &user, "verify").await?;
    }
    let jar = auth::clear_cookie(&app, jar, &auth::aux_name(&app, "signup"));
    let jar = auth::new_session(&app, &mut tx, &user, jar, &headers, false).await?;
    tx.commit().await?;
    eprintln!(
        "auth_event=oauth_signup provider={} outcome=created",
        pending.provider
    );
    Ok((jar, Json(json!({"signed_in":true}))))
}
pub async fn unlink(
    State(app): State<App>,
    jar: CookieJar,
    Path(name): Path<String>,
    Json(code): Json<auth::Code>,
) -> Result<Json<Value>> {
    let (mut tx, user, session) = auth::sensitive(&app, &jar, &code.code).await?;
    let methods: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE user_id=$1 AND provider<>$2")
            .bind(&user.id)
            .bind(&name)
            .fetch_one(&mut *tx)
            .await?;
    if methods == 0 && user.password_hash.is_none() {
        return Err(Error::bad(
            "Keep at least one sign-in method. Add a password or another provider first.",
        ));
    }
    let removed = sqlx::query("DELETE FROM identities WHERE user_id=$1 AND provider=$2")
        .bind(&user.id)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    if removed.rows_affected() != 1 {
        return Err(Error::bad("That provider is not linked to this account."));
    }
    auth::invalidate(&mut tx, &user.id, Some(&session.id)).await?;
    tx.commit().await?;
    Ok(Json(json!({"unlinked":true})))
}
