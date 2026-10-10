//! Open data (docs/CHANNEL_ADDITIONS.md "Open data"): weekly and monthly platform figures, written
//! once a day to `public_stats` and served with the privacy rules applied: no per-person or
//! per-channel numbers, weekly counts under 10 shown as "fewer than 10", and a month with fewer
//! than 10 paid creators folded into the next.
use crate::{
    App,
    profiles::{Fail, Res},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::header,
    response::IntoResponse,
    routing::get,
};
use serde_json::{Value, json};

const WEEKLY: &[(&str, &str)] = &[
    (
        "accounts_total",
        "Accounts at the end of the week (deleted ones excluded)",
    ),
    ("accounts_new", "Accounts created that week"),
    ("channels_streamed", "Channels that went live that week"),
    (
        "hours_streamed",
        "Hours of broadcasts that started that week",
    ),
    (
        "rotation_reach",
        "Percent of streams live 10+ minutes that the fair rotation put in first place",
    ),
    ("faction_myria", "Myria members"),
    ("faction_aetheron", "Aetheron members"),
    ("faction_glint", "Glint members"),
];
const MONTHLY: &[(&str, &str)] = &[
    (
        "creators_cents",
        "Subscriptions, gift subs and tributes: the creators' share, in cents",
    ),
    (
        "platform_cents",
        "The same money: S.V.E.R's share, in cents",
    ),
    ("payouts_cents", "Payouts sent to creators, in cents"),
];

/// Recomputes the last 12 weeks and 12 months, at most once a day (the jobs loop calls it).
pub async fn tick(app: &App) -> Res<()> {
    let fresh: bool = sqlx::query_scalar(
        "SELECT coalesce(max(computed_at)>now()-interval '1 day',false) FROM public_stats",
    )
    .fetch_one(&app.db)
    .await?;
    if !fresh {
        compute(app).await?;
    }
    Ok(())
}
/// Writes the figures (also run by the tests).
pub async fn compute(app: &App) -> Res<()> {
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO public_stats(period,metric,value) SELECT to_char(w,'IYYY-\"W\"IW'),m.metric,m.value FROM generate_series(date_trunc('week',now())-interval '11 weeks',date_trunc('week',now()),interval '1 week') w
        CROSS JOIN LATERAL (VALUES
          ('accounts_total',(SELECT count(*) FROM users WHERE created_at<w+interval '1 week' AND deleted_at IS NULL)::float8),
          ('accounts_new',(SELECT count(*) FROM users WHERE created_at>=w AND created_at<w+interval '1 week')::float8),
          ('channels_streamed',(SELECT count(DISTINCT owner_id) FROM broadcasts WHERE confirmed_live_at>=w AND confirmed_live_at<w+interval '1 week')::float8),
          ('hours_streamed',(SELECT coalesce(sum(extract(epoch FROM coalesce(ended_at,now())-started_at)),0)/3600 FROM broadcasts WHERE confirmed_live_at>=w AND confirmed_live_at<w+interval '1 week')::float8),
          ('rotation_reach_base',(SELECT count(*) FROM broadcasts WHERE confirmed_live_at>=w AND confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())-confirmed_live_at>=interval '10 minutes')::float8),
          ('rotation_reach',(SELECT CASE WHEN count(*)=0 THEN 0 ELSE 100.0*count(f.broadcast_id)/count(*) END FROM broadcasts b LEFT JOIN rotation_firsts f ON f.broadcast_id=b.id WHERE b.confirmed_live_at>=w AND b.confirmed_live_at<w+interval '1 week' AND coalesce(b.ended_at,now())-b.confirmed_live_at>=interval '10 minutes')::float8),
          ('faction_myria',(SELECT count(*) FROM faction_members WHERE faction='myria' AND joined_at<w+interval '1 week')::float8),
          ('faction_aetheron',(SELECT count(*) FROM faction_members WHERE faction='aetheron' AND joined_at<w+interval '1 week')::float8),
          ('faction_glint',(SELECT count(*) FROM faction_members WHERE faction='glint' AND joined_at<w+interval '1 week')::float8)
        ) m(metric,value)
        ON CONFLICT(period,metric) DO UPDATE SET value=EXCLUDED.value,computed_at=now()")
        .execute(&mut *tx).await?;
    // Money in tenths of a cent: card payments at their price, Valor at 1 cent each; the creators'
    // share is what reached earnings accounts. ponytail: refunds and chargebacks aren't netted
    // out; subtract them if they become material.
    sqlx::query("INSERT INTO public_stats(period,metric,value) SELECT to_char(m,'YYYY-MM'),x.metric,x.value FROM generate_series(date_trunc('month',now())-interval '11 months',date_trunc('month',now()),interval '1 month') m
        CROSS JOIN LATERAL (SELECT
          coalesce(sum(CASE WHEN t.kind IN ('sub_payment','gift_purchase') THEN (t.detail->>'cents')::bigint*10 ELSE (t.detail->>'valor')::bigint*10 END),0)::float8 AS gross,
          coalesce(sum((SELECT sum(e.amount) FROM ledger_entries e WHERE e.transaction_id=t.id AND e.account LIKE 'usd:earnings:%' AND e.amount>0)),0)::float8 AS creators
          FROM ledger_transactions t WHERE t.kind IN ('sub_payment','gift_purchase','tribute','valor_sub','valor_gift') AND t.created_at>=m AND t.created_at<m+interval '1 month') money
        CROSS JOIN LATERAL (VALUES
          ('creators_cents',money.creators/10),
          ('platform_cents',(money.gross-money.creators)/10),
          ('payouts_cents',(SELECT coalesce(sum(amount_cents),0) FROM payout_runs WHERE status='paid' AND completed_at>=m AND completed_at<m+interval '1 month')::float8),
          ('creators_paid',(SELECT count(DISTINCT user_id) FROM payout_runs WHERE status='paid' AND completed_at>=m AND completed_at<m+interval '1 month')::float8)
        ) x(metric,value)
        ON CONFLICT(period,metric) DO UPDATE SET value=EXCLUDED.value,computed_at=now()")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn rows(app: &App) -> Res<Vec<(String, String, f64)>> {
    Ok(
        sqlx::query_as("SELECT period,metric,value FROM public_stats ORDER BY period,metric")
            .fetch_all(&app.db)
            .await?,
    )
}
/// The published figures with the privacy rules applied.
async fn figures(app: &App) -> Res<(Vec<Value>, Vec<Value>)> {
    let all = rows(app).await?;
    let get = |period: &str, metric: &str| {
        all.iter()
            .find(|(p, m, _)| p == period && m == metric)
            .map(|r| r.2)
    };
    let mut periods: Vec<&String> = all.iter().map(|r| &r.0).collect();
    periods.dedup();
    let mut weekly = Vec::new();
    for period in periods.iter().filter(|p| p.contains('W')) {
        let mut row = json!({"period": period});
        for (metric, _) in WEEKLY {
            let value = get(period, metric).unwrap_or(0.0);
            // Counts under 10 are "fewer than 10"; the reach percentage needs 10 streams behind it.
            let base = if *metric == "rotation_reach" {
                get(period, "rotation_reach_base").unwrap_or(0.0)
            } else {
                value
            };
            row[*metric] = if base < 10.0 && *metric != "hours_streamed" {
                Value::Null
            } else {
                json!((value * 10.0).round() / 10.0)
            };
        }
        weekly.push(row);
    }
    let mut monthly = Vec::new();
    let mut carried = [0.0f64; 3];
    let mut paid = 0.0;
    let mut since: Option<&String> = None;
    for period in periods.iter().filter(|p| !p.contains('W')) {
        for (i, (metric, _)) in MONTHLY.iter().enumerate() {
            carried[i] += get(period, metric).unwrap_or(0.0);
        }
        paid += get(period, "creators_paid").unwrap_or(0.0);
        since.get_or_insert(period);
        // Fewer than 10 creators paid: fold this month into the next, so no one's pay shows.
        if paid >= 10.0 {
            let mut row = json!({"period": period, "from": since});
            for (i, (metric, _)) in MONTHLY.iter().enumerate() {
                row[*metric] = json!(carried[i].round());
            }
            monthly.push(row);
            carried = [0.0; 3];
            paid = 0.0;
            since = None;
        }
    }
    Ok((weekly, monthly))
}
/// GET /api/open-data
async fn open_data(State(app): State<App>) -> Res<Json<Value>> {
    let (weekly, monthly) = figures(&app).await?;
    let notes: Value = WEEKLY
        .iter()
        .chain(MONTHLY)
        .map(|(m, n)| (m.to_string(), json!(n)))
        .collect::<serde_json::Map<_, _>>()
        .into();
    Ok(Json(
        json!({"weekly": weekly, "monthly": monthly, "notes": notes}),
    ))
}
/// GET /api/open-data/{weekly|monthly}.csv: the same numbers as the charts.
async fn csv(State(app): State<App>, Path(name): Path<String>) -> Res<impl IntoResponse> {
    let (weekly, monthly) = figures(&app).await?;
    let (rows, metrics) = match name.as_str() {
        "weekly.csv" => (weekly, WEEKLY),
        "monthly.csv" => (monthly, MONTHLY),
        _ => return Err(Fail::missing()),
    };
    let mut out = format!(
        "period,{}\n",
        metrics.iter().map(|m| m.0).collect::<Vec<_>>().join(",")
    );
    for row in rows {
        let cells: Vec<String> = metrics
            .iter()
            .map(|(m, _)| match &row[*m] {
                Value::Null => "fewer than 10".into(),
                v => v.to_string(),
            })
            .collect();
        out.push_str(&format!(
            "{},{}\n",
            row["period"].as_str().unwrap_or(""),
            cells.join(",")
        ));
    }
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/open-data", get(open_data))
        .route("/api/open-data/{name}", get(csv))
}
