//! Ticketed playback from private storage: the two watermarked renditions for everyone allowed to
//! watch, the clean copy for its creator only, and the thumbnail.
use super::*;
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

#[derive(Deserialize)]
struct Link {
    ticket: String,
    #[serde(default)]
    q: String,
}
async fn file(
    State(app): State<App>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (beacon, ticket) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    let clean = link.q == "clean";
    if clean != (ticket.scope == "clean") {
        return Err(Fail::missing());
    }
    let key = match link.q.as_str() {
        "hd" => beacon.mp4_key,
        "sd" => beacon.mp4_small_key,
        "clean" => beacon.clean_key,
        _ => None,
    }
    .ok_or_else(Fail::missing)?;
    let bytes: Option<i64> = sqlx::query_scalar(
        "SELECT bytes FROM beacon_objects WHERE beacon_id=$1 AND key=$2 AND ready AND deleted_at IS NULL",
    )
    .bind(&id)
    .bind(&key)
    .fetch_optional(&app.db)
    .await?;
    let length = bytes.ok_or_else(Fail::missing)? as u64;
    let (start, end, partial) = crate::videos::delivery::range(
        headers.get(header::RANGE).and_then(|h| h.to_str().ok()),
        length,
    )?;
    let stream =
        futures_util::stream::try_unfold((app, key, start), move |(app, key, at)| async move {
            if at > end {
                return Ok::<_, std::io::Error>(None);
            }
            let until = (at + 1024 * 1024 - 1).min(end);
            let bytes = app
                .config
                .videos
                .storage
                .range(&app.http, &key, at, until)
                .await
                .map_err(|_| std::io::Error::other("Playback storage unavailable"))?;
            Ok(Some((Bytes::from(bytes), (app, key, until + 1))))
        });
    let mut response = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, (end - start + 1).to_string());
    if partial {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{length}"),
        );
    }
    if clean {
        response = response.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"sver-beacon-{id}.mp4\""),
        );
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|_| Fail::internal())
}
async fn thumbnail(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (beacon, ticket) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    if ticket.scope == "clean" {
        return Err(Fail::missing());
    }
    let key = beacon.thumbnail_key.as_deref().ok_or_else(Fail::missing)?;
    let bytes = app
        .config
        .videos
        .storage
        .get(&app.http, key)
        .await?
        .ok_or_else(Fail::missing)?;
    Ok(([(header::CONTENT_TYPE, "image/webp")], bytes).into_response())
}
pub fn routes() -> axum::Router<App> {
    axum::Router::new()
        .route("/api/beacons/{id}/file", get(file))
        .route("/api/beacons/{id}/thumbnail", get(thumbnail))
}
