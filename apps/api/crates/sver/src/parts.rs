//! SVER's own setup parts list, the parts picker search, and the staff review queue for custom
//! entries (docs/PROFILES.md, "Setup parts picker"). No prices or store links.
#![allow(clippy::type_complexity)]
use crate::{
    App,
    profiles::{Fail, Res, new_id, signed_in},
    safety, text,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;

/// The picker's categories, in display order. `OTHER` exists only for setup entries kept from
/// before the picker; nobody can add new ones.
pub const CATEGORIES: &[&str] = &[
    "CPU",
    "GPU",
    "RAM",
    "MOTHERBOARD",
    "CAMERA",
    "MIC",
    "PERIPHERALS",
];
pub const OTHER: &str = "OTHER";
pub const RESULTS: i64 = 10;
/// New queue entries one user can create per 24 hours; later custom entries are still saved.
pub const DAILY_SUBMISSIONS: i64 = 30;

#[derive(Deserialize)]
pub struct SearchQuery {
    category: String,
    #[serde(default)]
    q: String,
}
/// GET /api/parts?category=&q= (signed in): up to 10 active parts whose name contains every
/// typed word; an empty query lists the most common ones.
pub async fn search(
    State(app): State<App>,
    jar: CookieJar,
    Query(query): Query<SearchQuery>,
) -> Res<Json<Value>> {
    signed_in(&app, &jar).await?;
    if !CATEGORIES.contains(&query.category.as_str()) {
        return Err(Fail::field("category", "Choose a category."));
    }
    let q: String = query.q.chars().take(80).collect();
    let norm = text::part_norm(&q);
    let tokens: Vec<String> = norm
        .split(' ')
        .filter(|t| !t.is_empty())
        .take(8)
        .map(String::from)
        .collect();
    let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id,brand,model,kind FROM parts p WHERE category=$1 AND status='ACTIVE' AND NOT EXISTS (SELECT 1 FROM unnest($2::text[]) t WHERE position(t IN p.search)=0) ORDER BY (p.norm=$3) DESC,(p.norm LIKE $3||'%' OR replace(p.norm,' ','') LIKE replace($3,' ','')||'%') DESC,(replace(p.norm,' ','') LIKE '%'||replace($3,' ','')) DESC,(position(' '||$3 IN ' '||p.norm)>0) DESC,CASE WHEN $3='' THEN 0 ELSE length(p.norm) END,rank,brand,model LIMIT $4",
    )
    .bind(&query.category)
    .bind(&tokens)
    .bind(&norm)
    .bind(RESULTS)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!({
        "category": query.category,
        "parts": rows.iter().map(|(id, brand, model, kind)| json!({"id": id, "brand": brand, "model": model, "name": format!("{brand} {model}"), "kind": kind})).collect::<Vec<_>>(),
    })))
}

/// How one setup item is stored: the display name, the catalog part, and the review row.
pub struct Resolved {
    pub name: String,
    pub part_id: Option<String>,
    pub submission_id: Option<String>,
}
/// Resolves a setup item for saving. A picked part (or a typed name that exactly matches one)
/// uses the catalog name. Anything else is a custom entry: saved as typed and linked to the
/// shared review row for that name, created PENDING if it doesn't exist yet.
pub async fn resolve(
    db: &mut PgConnection,
    user_id: &str,
    category: &str,
    name: &str,
    part_id: Option<&str>,
) -> Res<Resolved> {
    if let Some(id) = part_id {
        let part: Option<(String, String)> = sqlx::query_as(
            "SELECT brand,model FROM parts WHERE id=$1 AND category=$2 AND status='ACTIVE'",
        )
        .bind(id)
        .bind(category)
        .fetch_optional(&mut *db)
        .await?;
        let (brand, model) =
            part.ok_or(Fail::field("part_id", "Choose a part from the list again."))?;
        return Ok(Resolved {
            name: format!("{brand} {model}"),
            part_id: Some(id.into()),
            submission_id: None,
        });
    }
    let norm = text::part_norm(name);
    if category == OTHER || norm.is_empty() {
        return Ok(Resolved {
            name: name.into(),
            part_id: None,
            submission_id: None,
        });
    }
    let exact: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id,brand,model FROM parts WHERE category=$1 AND norm=$2 AND status='ACTIVE'",
    )
    .bind(category)
    .bind(&norm)
    .fetch_optional(&mut *db)
    .await?;
    if let Some((id, brand, model)) = exact {
        return Ok(Resolved {
            name: format!("{brand} {model}"),
            part_id: Some(id),
            submission_id: None,
        });
    }
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM part_submissions WHERE category=$1 AND norm=$2")
            .bind(category)
            .bind(&norm)
            .fetch_optional(&mut *db)
            .await?;
    if existing.is_some() {
        return Ok(Resolved {
            name: name.into(),
            part_id: None,
            submission_id: existing,
        });
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM part_submissions WHERE submitted_by=$1 AND created_at>now()-interval '24 hours'",
    )
    .bind(user_id)
    .fetch_one(&mut *db)
    .await?;
    if recent >= DAILY_SUBMISSIONS {
        return Ok(Resolved {
            name: name.into(),
            part_id: None,
            submission_id: None,
        });
    }
    sqlx::query("INSERT INTO part_submissions(id,category,name,norm,submitted_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT (category,norm) DO NOTHING")
        .bind(new_id())
        .bind(category)
        .bind(name)
        .bind(&norm)
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    let id: String =
        sqlx::query_scalar("SELECT id FROM part_submissions WHERE category=$1 AND norm=$2")
            .bind(category)
            .bind(&norm)
            .fetch_one(&mut *db)
            .await?;
    Ok(Resolved {
        name: name.into(),
        part_id: None,
        submission_id: Some(id),
    })
}

// ---- Staff review queue ----

#[derive(Deserialize)]
pub struct QueueQuery {
    status: Option<String>,
}
/// GET /api/admin/parts?status=PENDING|APPROVED|DISMISSED (staff): custom entries, oldest
/// pending first, with how many setups use each and the closest catalog parts.
pub async fn admin_queue(
    State(app): State<App>,
    jar: CookieJar,
    Query(query): Query<QueueQuery>,
) -> Res<Json<Value>> {
    safety::staff(&app, &jar).await?;
    let status = query.status.unwrap_or_else(|| "PENDING".into());
    if !["PENDING", "APPROVED", "DISMISSED"].contains(&status.as_str()) {
        return Err(Fail::bad("Unknown status."));
    }
    let mut db = app.db.acquire().await?;
    let rows: Vec<(String, String, String, String, DateTime<Utc>, Option<String>, i64, Option<String>, Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT s.id,s.category,s.name,s.norm,s.created_at,(SELECT username FROM users WHERE id=s.submitted_by),(SELECT count(*) FROM setup_items i WHERE i.submission_id=s.id),(SELECT brand||' '||model FROM parts WHERE id=s.part_id),(SELECT username FROM users WHERE id=s.reviewed_by),s.reviewed_at FROM part_submissions s WHERE s.status=$1 ORDER BY CASE WHEN $1='PENDING' THEN s.created_at END ASC, s.reviewed_at DESC NULLS LAST LIMIT 100",
    )
    .bind(&status)
    .fetch_all(&mut *db)
    .await?;
    let mut items = Vec::new();
    for r in rows {
        let first = r.3.split(' ').next().unwrap_or("").to_string();
        let similar: Vec<String> = sqlx::query_scalar(
            "SELECT brand||' '||model FROM parts WHERE category=$1 AND status='ACTIVE' AND search LIKE '%'||$2||'%' ORDER BY rank LIMIT 3",
        )
        .bind(&r.1)
        .bind(&first)
        .fetch_all(&mut *db)
        .await?;
        items.push(json!({
            "id": r.0, "category": r.1, "name": r.2, "created_at": r.4, "submitted_by": r.5,
            "uses": r.6, "part": r.7, "reviewed_by": r.8, "reviewed_at": r.9, "similar": similar,
        }));
    }
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM part_submissions WHERE status='PENDING'")
            .fetch_one(&mut *db)
            .await?;
    Ok(Json(
        json!({"status": status, "pending": pending, "items": items, "categories": CATEGORIES}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    decision: String,
    #[serde(default)]
    brand: String,
    #[serde(default)]
    model: String,
}
/// POST /api/admin/parts/{id}/decision (staff, step-up): `approve` with the catalog brand and
/// model (staff may tidy the spelling), or `dismiss`. Approving adds the part (or reuses the one
/// with the same normalized name) and links every setup entry that used this custom name.
/// Dismissing keeps those entries as they are. Both are audited.
pub async fn admin_decide(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(input): Json<Decision>,
) -> Res<Json<Value>> {
    let staff = safety::staff_write(&app, &jar).await?;
    let mut tx = app.db.begin().await?;
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT category,name,status FROM part_submissions WHERE id=$1 FOR UPDATE")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await?;
    let (category, name, status) = row.ok_or_else(Fail::missing)?;
    if status != "PENDING" {
        return Err(Fail::conflict("This entry was already reviewed."));
    }
    match input.decision.as_str() {
        "approve" => {
            let brand = text::plain(&input.brand, "brand", 1, 40, 0, true)?;
            let model = text::plain(&input.model, "model", 1, 80, 0, true)?;
            let display = format!("{brand} {model}");
            let norm = text::part_norm(&display);
            if norm.is_empty() {
                return Err(Fail::field("model", "Enter a part name."));
            }
            let existing: Option<(String, String)> = sqlx::query_as(
                "SELECT id,brand||' '||model FROM parts WHERE category=$1 AND norm=$2",
            )
            .bind(&category)
            .bind(&norm)
            .fetch_optional(&mut *tx)
            .await?;
            let (part_id, part_name, added) = match existing {
                Some((pid, pname)) => {
                    sqlx::query("UPDATE parts SET status='ACTIVE' WHERE id=$1")
                        .bind(&pid)
                        .execute(&mut *tx)
                        .await?;
                    (pid, pname, false)
                }
                None => {
                    let pid = format!("part_{}", new_id().replace('-', ""));
                    sqlx::query("INSERT INTO parts(id,category,brand,model,norm,source) VALUES($1,$2,$3,$4,$5,'STAFF')")
                        .bind(&pid)
                        .bind(&category)
                        .bind(&brand)
                        .bind(&model)
                        .bind(&norm)
                        .execute(&mut *tx)
                        .await?;
                    (pid, display, true)
                }
            };
            sqlx::query("UPDATE part_submissions SET status='APPROVED',part_id=$2,reviewed_by=$3,reviewed_at=now() WHERE id=$1")
                .bind(&id)
                .bind(&part_id)
                .bind(&staff.id)
                .execute(&mut *tx)
                .await?;
            let linked =
                sqlx::query("UPDATE setup_items SET part_id=$2,name=$3 WHERE submission_id=$1")
                    .bind(&id)
                    .bind(&part_id)
                    .bind(&part_name)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            safety::audit(&mut tx, Some(&staff.id), "part_approved", "part_submission", &id, &[], "", json!({"category": category, "name": name, "part_id": part_id, "part": part_name, "added": added, "linked": linked}), false).await?;
            tx.commit().await?;
            Ok(Json(
                json!({"status": "APPROVED", "part": {"id": part_id, "name": part_name}, "added": added, "linked": linked}),
            ))
        }
        "dismiss" => {
            sqlx::query("UPDATE part_submissions SET status='DISMISSED',reviewed_by=$2,reviewed_at=now() WHERE id=$1")
                .bind(&id)
                .bind(&staff.id)
                .execute(&mut *tx)
                .await?;
            safety::audit(
                &mut tx,
                Some(&staff.id),
                "part_dismissed",
                "part_submission",
                &id,
                &[],
                "",
                json!({"category": category, "name": name}),
                false,
            )
            .await?;
            tx.commit().await?;
            Ok(Json(json!({"status": "DISMISSED"})))
        }
        _ => Err(Fail::field("decision", "Choose approve or dismiss.")),
    }
}
