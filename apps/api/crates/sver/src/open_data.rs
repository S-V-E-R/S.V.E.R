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
        "Accounts open at the end of the week (accounts deleted later still count for the weeks they existed)",
    ),
    ("accounts_new", "Accounts created that week"),
    ("channels_streamed", "Channels live at any point that week"),
    (
        "hours_streamed",
        "Hours creators were live that week, counted in the week they happened (a stream that runs past Sunday splits across weeks); hidden until 10 creators streamed, so no one's hours show",
    ),
    (
        "hours_always_on",
        "Hours of S.V.E.R's own 24/7 channel (SVER Plays) that week",
    ),
    (
        "rotation_reach",
        "Percent of streams live 10+ minutes that the fair rotation put in first place",
    ),
    (
        "faction_myria",
        "Myria members at the end of the week, shown exactly: the member directory is public",
    ),
    (
        "faction_aetheron",
        "Aetheron members at the end of the week, shown exactly: the member directory is public",
    ),
    (
        "faction_glint",
        "Glint members at the end of the week, shown exactly: the member directory is public",
    ),
    (
        "watch_hours",
        "Hours real viewers watched that week (sessions viewer integrity counted); these viewer figures are hidden until 10 channels had viewers",
    ),
    (
        "discovery_watch_share",
        "Percent of that watch time that came from discovery: the homepage, MAGNet, Browse, Search, suggestions and faction pages",
    ),
    ("avg_visit_minutes", "Average length of a visit, in minutes"),
    (
        "home_click_through",
        "Percent of times a live card was seen on the homepage that led to a visit",
    ),
    (
        "magnet_to_channel",
        "Percent of viewers who watched a stream inside MAGNet and then opened that channel",
    ),
    (
        "discovery_lift",
        "New arrivals per hour while a stream was on top of Live Now or featured on MAGNet, as a multiple of the rate the rest of the time",
    ),
];
const MONTHLY: &[(&str, &str)] = &[
    (
        "creators_cents",
        "Subscriptions, gift subs and tributes: the creators' share, in cents, less refunds and chargebacks",
    ),
    (
        "platform_cents",
        "The same money: S.V.E.R's share, in cents",
    ),
    ("payouts_cents", "Payouts sent to creators, in cents"),
    (
        "wall_cents",
        "Wall seats sold, in cents, less refunds (all of it goes to S.V.E.R)",
    ),
    (
        "supporters_cents",
        "Platform supporter subscriptions, in cents, less refunds (all of it goes to S.V.E.R)",
    ),
    (
        "ads_cents",
        "Ad revenue from verified impressions, in cents (before streamers' and viewers' shares)",
    ),
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
    // Viewer analytics (docs/ANALYTICS.md): `exp` is each channel's exposed time, on top of Live
    // Now or featured on MAGNet; `wv` real visits by ISO week.
    sqlx::query("WITH exp AS (SELECT b.owner_id AS o,to_timestamp(r.slot*180) AS s,to_timestamp((r.slot+1)*180) AS e FROM rotation_turns r JOIN broadcasts b ON b.id=r.broadcast_id
            UNION ALL SELECT owner_id,started_at,coalesce(ended_at,now()) FROM magnet_features),
        wv AS (SELECT v.*,date_trunc('week',v.started_at) AS wk FROM watch_visits v WHERE v.counted)
        INSERT INTO public_stats(period,metric,value) SELECT to_char(w,'IYYY-\"W\"IW'),m.metric,m.value FROM generate_series(date_trunc('week',now())-interval '11 weeks',date_trunc('week',now()),interval '1 week') w
        CROSS JOIN LATERAL (VALUES
          ('accounts_total',(SELECT count(*) FROM users WHERE created_at<w+interval '1 week' AND (deleted_at IS NULL OR deleted_at>=w+interval '1 week'))::float8),
          ('accounts_new',(SELECT count(*) FROM users WHERE created_at>=w AND created_at<w+interval '1 week')::float8),
          ('channels_streamed',(SELECT count(DISTINCT owner_id) FROM broadcasts WHERE confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())>w)::float8),
          ('hours_streamed',(SELECT coalesce(sum(extract(epoch FROM least(coalesce(ended_at,now()),w+interval '1 week')-greatest(confirmed_live_at,w))),0)/3600 FROM broadcasts WHERE confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())>w AND NOT EXISTS(SELECT 1 FROM plays_runtime p WHERE p.channel_id=owner_id))::float8),
          ('hours_streamed_base',(SELECT count(DISTINCT owner_id) FROM broadcasts WHERE confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())>w AND NOT EXISTS(SELECT 1 FROM plays_runtime p WHERE p.channel_id=owner_id))::float8),
          ('hours_always_on',(SELECT coalesce(sum(extract(epoch FROM least(coalesce(ended_at,now()),w+interval '1 week')-greatest(confirmed_live_at,w))),0)/3600 FROM broadcasts WHERE confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())>w AND EXISTS(SELECT 1 FROM plays_runtime p WHERE p.channel_id=owner_id))::float8),
          ('rotation_reach_base',(SELECT count(*) FROM broadcasts WHERE confirmed_live_at>=w AND confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())-confirmed_live_at>=interval '10 minutes')::float8),
          ('rotation_reach',(SELECT CASE WHEN count(*)=0 THEN 0 ELSE 100.0*count(f.broadcast_id)/count(*) END FROM broadcasts b LEFT JOIN rotation_firsts f ON f.broadcast_id=b.id WHERE b.confirmed_live_at>=w AND b.confirmed_live_at<w+interval '1 week' AND coalesce(b.ended_at,now())-b.confirmed_live_at>=interval '10 minutes')::float8),
          ('faction_myria',(SELECT count(*) FROM faction_members f JOIN users u ON u.id=f.user_id WHERE faction='myria' AND joined_at<w+interval '1 week' AND (u.deleted_at IS NULL OR u.deleted_at>=w+interval '1 week'))::float8),
          ('faction_aetheron',(SELECT count(*) FROM faction_members f JOIN users u ON u.id=f.user_id WHERE faction='aetheron' AND joined_at<w+interval '1 week' AND (u.deleted_at IS NULL OR u.deleted_at>=w+interval '1 week'))::float8),
          ('faction_glint',(SELECT count(*) FROM faction_members f JOIN users u ON u.id=f.user_id WHERE faction='glint' AND joined_at<w+interval '1 week' AND (u.deleted_at IS NULL OR u.deleted_at>=w+interval '1 week'))::float8),
          ('analytics_base',(SELECT count(DISTINCT channel_id) FROM wv WHERE wk=w)::float8),
          ('watch_hours',(SELECT coalesce(sum(seconds),0)/3600 FROM wv WHERE wk=w)::float8),
          ('discovery_watch_share',(SELECT coalesce(100*sum(seconds) FILTER (WHERE source IN ('home','magnet','magnet_channel','browse','search','suggestion','faction'))/nullif(sum(seconds),0),0) FROM wv WHERE wk=w)::float8),
          ('avg_visit_minutes',(SELECT coalesce(avg(seconds),0)/60 FROM wv WHERE wk=w)::float8),
          ('home_click_through',(SELECT coalesce(100.0*(SELECT count(*) FROM wv WHERE wk=w AND source='home')/nullif((SELECT sum(shown) FROM discovery_impressions WHERE surface='home' AND hour>=w AND hour<w+interval '1 week'),0),0))::float8),
          ('magnet_to_channel',(SELECT coalesce(100.0*count(DISTINCT (a.channel_id,a.viewer_key)) FILTER (WHERE EXISTS(SELECT 1 FROM wv c WHERE c.source='magnet' AND c.channel_id=a.channel_id AND c.viewer_key=a.viewer_key AND c.started_at>=a.started_at AND c.wk=w))/nullif(count(DISTINCT (a.channel_id,a.viewer_key)),0),0) FROM wv a WHERE a.wk=w AND a.source='magnet_channel')::float8),
          ('discovery_lift',(SELECT CASE WHEN eh>0 AND lh>eh AND va>ea THEN (ea/eh)/((va-ea)/(lh-eh)) ELSE 0 END FROM (SELECT
              (SELECT count(*) FROM wv v WHERE v.wk=w AND EXISTS(SELECT 1 FROM exp x WHERE x.o=v.channel_id AND v.started_at>=x.s AND v.started_at<x.e))::float8 AS ea,
              (SELECT coalesce(sum(extract(epoch FROM least(x.e,w+interval '1 week')-greatest(x.s,w))),0)/3600 FROM exp x WHERE x.s<w+interval '1 week' AND x.e>w)::float8 AS eh,
              (SELECT count(*) FROM wv WHERE wk=w)::float8 AS va,
              (SELECT coalesce(sum(extract(epoch FROM least(coalesce(ended_at,now()),w+interval '1 week')-greatest(confirmed_live_at,w))),0)/3600 FROM broadcasts WHERE confirmed_live_at<w+interval '1 week' AND coalesce(ended_at,now())>w)::float8 AS lh) q))
        ) m(metric,value)
        ON CONFLICT(period,metric) DO UPDATE SET value=EXCLUDED.value,computed_at=now()")
        .execute(&mut *tx).await?;
    // Money in tenths of a cent: card payments at their price, Valor at 1 cent each; the creators'
    // share is what reached earnings accounts. Refunds and chargebacks of card payments (and a won
    // dispute's money coming back) count in the month they happen, found by payment intent: they
    // post the original payment's pairs scaled, so their cash and earnings entries are the net.
    sqlx::query("INSERT INTO public_stats(period,metric,value) SELECT to_char(m,'YYYY-MM'),x.metric,x.value FROM generate_series(date_trunc('month',now())-interval '11 months',date_trunc('month',now()),interval '1 month') m
        CROSS JOIN LATERAL (SELECT
          coalesce(sum(CASE WHEN t.kind IN ('sub_payment','gift_purchase') THEN (t.detail->>'cents')::bigint*10 ELSE (t.detail->>'valor')::bigint*10 END),0)::float8 AS gross,
          coalesce(sum((SELECT sum(e.amount) FROM ledger_entries e WHERE e.transaction_id=t.id AND e.account LIKE 'usd:earnings:%' AND e.amount>0)),0)::float8 AS creators
          FROM ledger_transactions t WHERE t.kind IN ('sub_payment','gift_purchase','tribute','valor_sub','valor_gift') AND t.created_at>=m AND t.created_at<m+interval '1 month') money
        CROSS JOIN LATERAL (SELECT
          coalesce(sum(r.cash) FILTER (WHERE r.of IN ('sub_payment','gift_purchase')),0)::float8 AS card,
          coalesce(sum(r.creators) FILTER (WHERE r.of IN ('sub_payment','gift_purchase')),0)::float8 AS creators,
          coalesce(sum(r.cash) FILTER (WHERE r.of='wall_purchase'),0)::float8 AS wall,
          coalesce(sum(r.cash) FILTER (WHERE r.of='platform_sub'),0)::float8 AS supporters
          FROM (SELECT o.kind AS of,
              (SELECT coalesce(sum(e.amount),0) FROM ledger_entries e WHERE e.transaction_id=t.id AND e.account='usd:stripe') AS cash,
              (SELECT coalesce(sum(e.amount),0) FROM ledger_entries e WHERE e.transaction_id=t.id AND e.account LIKE 'usd:earnings:%') AS creators
            FROM ledger_transactions t
            JOIN LATERAL (SELECT o.kind FROM ledger_transactions o WHERE o.kind IN ('sub_payment','gift_purchase','wall_purchase','platform_sub') AND o.detail->>'payment_intent'=t.detail->>'payment_intent' LIMIT 1) o ON true
            WHERE t.kind IN ('refund','dispute','dispute_won') AND t.created_at>=m AND t.created_at<m+interval '1 month') r) back
        CROSS JOIN LATERAL (VALUES
          ('creators_cents',(money.creators+back.creators)/10),
          ('platform_cents',(money.gross+back.card-money.creators-back.creators)/10),
          ('payouts_cents',(SELECT coalesce(sum(amount_cents),0) FROM payout_runs WHERE status='paid' AND completed_at>=m AND completed_at<m+interval '1 month')::float8),
          ('creators_paid',(SELECT count(DISTINCT user_id) FROM payout_runs WHERE status='paid' AND completed_at>=m AND completed_at<m+interval '1 month')::float8),
          ('supporters_cents',(SELECT coalesce(sum((detail->>'cents')::bigint),0) FROM ledger_transactions WHERE kind='platform_sub' AND created_at>=m AND created_at<m+interval '1 month')::float8+back.supporters/10),
          ('ads_cents',(SELECT coalesce(sum((detail->>'value')::bigint),0)/10 FROM ledger_transactions WHERE kind='ad_impression' AND created_at>=m AND created_at<m+interval '1 month')::float8),
          ('wall_cents',(SELECT coalesce(sum((detail->>'cents')::bigint),0) FROM ledger_transactions WHERE kind='wall_purchase' AND created_at>=m AND created_at<m+interval '1 month')::float8+back.wall/10)
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
/// The published figures with the privacy rules applied. The current week and month are marked
/// `complete: false` (they're still filling up).
async fn figures(app: &App) -> Res<(Vec<Value>, Vec<Value>)> {
    let now = chrono::Utc::now();
    let (this_week, this_month) = (
        now.format("%G-W%V").to_string(),
        now.format("%Y-%m").to_string(),
    );
    let all = rows(app).await?;
    let get = |period: &str, metric: &str| {
        all.iter()
            .find(|(p, m, _)| p == period && m == metric)
            .map(|r| r.2)
    };
    let mut periods: Vec<&String> = all.iter().map(|r| &r.0).collect();
    periods.dedup();
    let mut weekly = Vec::new();
    let weeks: Vec<&String> = periods
        .iter()
        .copied()
        .filter(|p| p.contains('W'))
        .collect();
    // Only the 12 weeks compute() refreshes; older rows were frozen under older definitions.
    for period in &weeks[weeks.len().saturating_sub(12)..] {
        let mut row = json!({"period": period, "complete": **period != this_week});
        for (metric, _) in WEEKLY {
            let value = get(period, metric).unwrap_or(0.0);
            // Counts under 10 are "fewer than 10"; the reach percentage needs 10 streams behind it,
            // creators' hours 10 creators. The 24/7 channel is S.V.E.R's own, so it always shows.
            let base = match *metric {
                "rotation_reach" | "hours_streamed" => {
                    get(period, &format!("{metric}_base")).unwrap_or(0.0)
                }
                // Faction membership is public in each faction's member directory.
                "hours_always_on" | "faction_myria" | "faction_aetheron" | "faction_glint" => 10.0,
                // Viewer figures need 10 channels with viewers behind them, so no one's show.
                "watch_hours"
                | "discovery_watch_share"
                | "avg_visit_minutes"
                | "home_click_through"
                | "magnet_to_channel"
                | "discovery_lift" => get(period, "analytics_base").unwrap_or(0.0),
                _ => value,
            };
            row[*metric] = if base < 10.0 {
                Value::Null
            } else {
                json!((value * 10.0).round() / 10.0)
            };
        }
        weekly.push(row);
    }
    let mut monthly = Vec::new();
    let mut carried = [0.0f64; MONTHLY.len()];
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
            let mut row =
                json!({"period": period, "from": since, "complete": **period != this_month});
            for (i, (metric, _)) in MONTHLY.iter().enumerate() {
                row[*metric] = json!(carried[i].round());
            }
            monthly.push(row);
            carried = [0.0; MONTHLY.len()];
            paid = 0.0;
            since = None;
        }
    }
    Ok((weekly, monthly))
}
/// GET /api/open-data
async fn open_data(State(app): State<App>) -> Res<Json<Value>> {
    let (weekly, monthly) = figures(&app).await?;
    let updated: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT max(computed_at) FROM public_stats")
            .fetch_one(&app.db)
            .await?;
    let notes: Value = WEEKLY
        .iter()
        .chain(MONTHLY)
        .map(|(m, n)| (m.to_string(), json!(n)))
        .collect::<serde_json::Map<_, _>>()
        .into();
    Ok(Json(
        json!({"weekly": weekly, "monthly": monthly, "notes": notes, "updated": updated}),
    ))
}
/// GET /api/open-data/{weekly|monthly}.csv: the same numbers as the charts.
async fn csv(State(app): State<App>, Path(name): Path<String>) -> Res<impl IntoResponse> {
    let (weekly, monthly) = figures(&app).await?;
    // A monthly row can combine several months: `from` is the first, `period` the last.
    let (rows, metrics, lead) = match name.as_str() {
        "weekly.csv" => (weekly, WEEKLY, &["period", "complete"][..]),
        "monthly.csv" => (monthly, MONTHLY, &["period", "from", "complete"][..]),
        _ => return Err(Fail::missing()),
    };
    let mut out = format!(
        "{},{}\n",
        lead.join(","),
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
        let leading: Vec<String> = lead
            .iter()
            .map(|c| match &row[*c] {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            })
            .collect();
        out.push_str(&format!("{},{}\n", leading.join(","), cells.join(",")));
    }
    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], out))
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/open-data", get(open_data))
        .route("/api/open-data/{name}", get(csv))
}
