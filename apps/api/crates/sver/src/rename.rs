//! Username renames, history and holds (docs/PROFILES.md, "Rename, history and hold").
use crate::{
    App, auth,
    auth::User,
    profiles::{Fail, Res},
    reserved, security as sec,
};
use axum::{Json, extract::State};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::str::FromStr;

pub const NOT_AVAILABLE: &str = "That username isn't available.";

/// True when an active rename or erasure hold covers this name (case-insensitive).
pub async fn held(db: &mut PgConnection, name: &str, except_user: Option<&str>) -> Res<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM username_holds WHERE handle_canonical=lower($1) AND released_at>now() AND (user_id IS NULL OR $2::text IS NULL OR user_id<>$2))")
        .bind(name)
        .bind(except_user)
        .fetch_one(&mut *db)
        .await?)
}
async fn last_rename(db: &mut PgConnection, user_id: &str) -> Res<Option<DateTime<Utc>>> {
    Ok(sqlx::query_scalar(
        "SELECT max(changed_at) FROM username_history WHERE user_id=$1 AND reason='rename'",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?)
}
/// Rename state for the settings page: the next allowed date and names held for this user.
pub async fn status(db: &mut PgConnection, user: &User) -> Res<Value> {
    let next = last_rename(db, &user.id)
        .await?
        .map(|at| at + Duration::days(60))
        .filter(|at| *at > Utc::now());
    let holds: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('name',h.handle_canonical,'released_at',h.released_at) FROM username_holds h WHERE h.user_id=$1 AND h.released_at>now() AND h.redirect ORDER BY h.released_at")
        .bind(&user.id)
        .fetch_all(&mut *db)
        .await?;
    Ok(json!({"next_allowed_at": next, "held_names": holds}))
}

#[derive(Deserialize)]
pub struct RenameInput {
    username: String,
    #[serde(default)]
    code: String,
    timezone: Option<String>,
}
fn format_date(at: DateTime<Utc>, zone: Option<&str>) -> String {
    let tz = zone
        .and_then(|z| Tz::from_str(z).ok())
        .unwrap_or(chrono_tz::UTC);
    at.with_timezone(&tz).format("%B %-d, %Y").to_string()
}
/// POST /api/me/username: sensitive change (recent sign-in plus a fresh MFA code when enabled).
pub async fn rename(
    State(app): State<App>,
    jar: CookieJar,
    Json(input): Json<RenameInput>,
) -> Res<Json<Value>> {
    let new = input.username.trim().to_string();
    sec::validate_username(&new).map_err(|e| {
        if e.1 == NOT_AVAILABLE {
            Fail::conflict(NOT_AVAILABLE)
        } else {
            Fail::field("username", e.1)
        }
    })?;
    let (mut tx, user, _) = auth::sensitive(&app, &jar, &input.code).await?;
    // The session query above holds the user-row lock for the rest of this transaction.
    if new == user.username {
        return Err(Fail::field("username", "That's already your username."));
    }
    let old = user.username.clone();
    if new.eq_ignore_ascii_case(&old) {
        // Case-only change: free, no interval, no hold, no history.
        sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
            .bind(&user.id)
            .bind(&new)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        eprintln!("profile_event=rename outcome=case_only");
        return Ok(Json(json!({"username": new, "hold": false})));
    }
    let own_hold: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM username_holds WHERE handle_canonical=lower($1) AND user_id=$2 AND released_at>now() AND redirect)")
        .bind(&new)
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
    if own_hold {
        // One-time switch back to a name still held for this user: ends that hold, isn't a rename.
        sqlx::query("DELETE FROM username_holds WHERE handle_canonical=lower($1) AND user_id=$2")
            .bind(&new)
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE lower(username)=lower($1) AND id<>$2)",
        )
        .bind(&new)
        .bind(&user.id)
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            return Err(Fail::conflict(NOT_AVAILABLE));
        }
        sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
            .bind(&user.id)
            .bind(&new)
            .execute(&mut *tx)
            .await
            .map_err(map_unique)?;
        sqlx::query("INSERT INTO username_history(id,user_id,old_username,new_username,reason) VALUES($1,$2,$3,$4,'revert')").bind(crate::profiles::new_id()).bind(&user.id).bind(&old).bind(&new).execute(&mut *tx).await?;
        tx.commit().await?;
        eprintln!("profile_event=rename outcome=revert");
        return Ok(Json(json!({"username": new, "hold": false})));
    }
    // Spec format additions apply to renames (and new signups) only.
    if let Some(last) = last_rename(&mut tx, &user.id).await? {
        let next = last + Duration::days(60);
        if next > Utc::now() {
            return Err(Fail::conflict(format!(
                "You can change your username again on {}.",
                format_date(next, input.timezone.as_deref())
            )));
        }
    }
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE lower(username)=lower($1) AND id<>$2)",
    )
    .bind(&new)
    .bind(&user.id)
    .fetch_one(&mut *tx)
    .await?;
    if taken || held(&mut tx, &new, Some(&user.id)).await? {
        return Err(Fail::conflict(NOT_AVAILABLE));
    }
    sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
        .bind(&user.id)
        .bind(&new)
        .execute(&mut *tx)
        .await
        .map_err(map_unique)?;
    sqlx::query("INSERT INTO username_history(id,user_id,old_username,new_username,reason) VALUES($1,$2,$3,$4,'rename')").bind(crate::profiles::new_id()).bind(&user.id).bind(&old).bind(&new).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO username_holds(handle_canonical,user_id,released_at,redirect) VALUES(lower($1),$2,now()+interval '30 days',true) ON CONFLICT (handle_canonical) DO UPDATE SET user_id=EXCLUDED.user_id,released_at=EXCLUDED.released_at,redirect=true,created_at=now()")
        .bind(&old)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    // A claim of the new name may also be held by an expired row; clear it so the name is clean.
    sqlx::query(
        "DELETE FROM username_holds WHERE handle_canonical=lower($1) AND released_at<=now()",
    )
    .bind(&new)
    .execute(&mut *tx)
    .await?;
    tx.commit().await.map_err(map_unique)?;
    eprintln!("profile_event=rename outcome=renamed");
    Ok(Json(
        json!({"username": new, "hold": true, "next_allowed_at": Utc::now() + Duration::days(60)}),
    ))
}
fn map_unique(e: sqlx::Error) -> Fail {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        Fail::conflict(NOT_AVAILABLE)
    } else {
        e.into()
    }
}
/// Name format check used by signup and rename (existing names are never revalidated).
pub fn shape_error(name: &str) -> Option<&'static str> {
    if !reserved::is_username_shaped(name) {
        Some("Username must be 3-25 letters, numbers or underscores.")
    } else if name.starts_with('_') || name.ends_with('_') {
        Some("Usernames can't start or end with an underscore.")
    } else if name.bytes().all(|b| b.is_ascii_digit()) {
        Some("Usernames can't be only numbers.")
    } else {
        None
    }
}

#[derive(Deserialize)]
pub struct ResetInput {
    /// Shown to the user on their standing page.
    reason: String,
}
/// How long an impersonating name stays held (without a redirect) after a staff reset.
const RESET_HOLD_DAYS: i64 = 90;

/// POST /api/admin/users/{username}/username-reset: staff replace an impersonating username with a
/// neutral one (`user` + 8 digits). The account keeps its identity, follows and content. The old
/// name is held without a redirect, so it neither points at the account nor can be reclaimed by
/// anyone while the hold lasts. A reset doesn't count toward the user's own 60-day rename limit.
pub async fn staff_reset(
    State(app): State<App>,
    jar: CookieJar,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(input): Json<ResetInput>,
) -> Res<Json<Value>> {
    let actor = crate::safety::staff_write(&app, &jar).await?;
    let reason = crate::safety::note(Some(&input.reason), "reason", true)?;
    let mut tx = app.db.begin().await?;
    let user_id = crate::safety::admin_target(&mut tx, &name).await?;
    // The account lifecycle lock serializes this with sign-in, renames and stream starts.
    let user = auth::stream_owner(&mut tx, &user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    let target = crate::profiles::channel_user_by_id(&mut tx, &user_id)
        .await?
        .ok_or_else(Fail::missing)?;
    if target.internal || target.deleted_at.is_some() {
        return Err(Fail::bad("This account's username can't be reset."));
    }
    if crate::safety::is_staff(&mut tx, &user_id).await? {
        return Err(Fail::bad(
            "Remove the staff role before resetting this username.",
        ));
    }
    let old = user.username.clone();
    let mut new = None;
    for _ in 0..20 {
        let candidate = format!("user{:08}", uuid::Uuid::new_v4().as_u128() % 100_000_000);
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE lower(username)=lower($1))",
        )
        .bind(&candidate)
        .fetch_one(&mut *tx)
        .await?;
        if shape_error(&candidate).is_none()
            && !reserved::is_reserved(&candidate)
            && !taken
            && !held(&mut tx, &candidate, None).await?
        {
            new = Some(candidate);
            break;
        }
    }
    let new = new.ok_or_else(|| Fail::unavailable("No replacement name was free. Try again."))?;
    sqlx::query("UPDATE users SET username=$2 WHERE id=$1")
        .bind(&user_id)
        .bind(&new)
        .execute(&mut *tx)
        .await
        .map_err(map_unique)?;
    sqlx::query("INSERT INTO username_history(id,user_id,old_username,new_username,reason,note) VALUES($1,$2,$3,$4,'staff_reset',$5)")
        .bind(crate::profiles::new_id()).bind(&user_id).bind(&old).bind(&new).bind(&reason)
        .execute(&mut *tx).await?;
    // Any redirect the account still had (earlier names) stops pointing at it, and the
    // impersonating name is held, unredirected, for everyone.
    sqlx::query("UPDATE username_holds SET redirect=false WHERE user_id=$1")
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO username_holds(handle_canonical,user_id,released_at,redirect) VALUES(lower($1),$2,now()+make_interval(days=>$3),false) ON CONFLICT (handle_canonical) DO UPDATE SET user_id=EXCLUDED.user_id,released_at=EXCLUDED.released_at,redirect=false,created_at=now()")
        .bind(&old).bind(&user_id).bind(RESET_HOLD_DAYS as i32)
        .execute(&mut *tx).await?;
    crate::safety::audit(
        &mut tx,
        Some(&actor.id),
        "username_reset",
        "user",
        &user_id,
        &[],
        &reason,
        json!({"old": old, "new": new}),
        false,
    )
    .await?;
    crate::safety::queue_notice(
        &app,
        &mut tx,
        &user_id,
        "Your S.V.E.R account standing",
        crate::safety::STANDING_MAIL,
    )
    .await?;
    tx.commit().await.map_err(map_unique)?;
    crate::safety::log("username_reset", "ok");
    Ok(Json(json!({"username": new})))
}
