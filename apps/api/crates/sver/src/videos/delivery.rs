use super::*;
use axum::body::Bytes;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};

#[derive(Deserialize)]
struct Link {
    ticket: String,
}
#[derive(sqlx::FromRow)]
struct Segment {
    object_key: String,
    duration_ms: i64,
    discontinuity: bool,
}
async fn playlist(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (video, ticket) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    if !matches!(ticket.scope.as_str(), "play" | "review")
        || video.kind == "CLIP"
        || (!video.recording && ticket.scope != "review")
    {
        return Err(Fail::missing());
    }
    // EVENT playlists may append, but must never prepend recovered segments or skip a gap.
    let segments:Vec<Segment>=sqlx::query_as("WITH pending AS (SELECT min(start_ms) AS start_ms FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND NOT o.ready) SELECT object_key,duration_ms,discontinuity FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND o.ready AND s.start_ms<coalesce((SELECT start_ms FROM pending),9223372036854775807) ORDER BY s.start_ms").bind(&id).fetch_all(&app.db).await?;
    if segments.is_empty() {
        return Err(Fail::unavailable("This recording is still processing."));
    }
    let target = (segments.iter().map(|s| s.duration_ms).max().unwrap_or(1000) + 999) / 1000;
    let token: String = url::form_urlencoded::byte_serialize(link.ticket.as_bytes()).collect();
    let mut body = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:{}\n",
        if video.status == "RECORDING" {
            "EVENT"
        } else {
            "VOD"
        }
    );
    for segment in segments {
        if segment.discontinuity {
            body.push_str("#EXT-X-DISCONTINUITY\n");
        }
        body.push_str(&format!(
            "#EXTINF:{:.3},\n/api/videos/{id}/segments/{}?ticket={token}\n",
            segment.duration_ms as f64 / 1000.0,
            segment
                .object_key
                .rsplit('/')
                .next()
                .ok_or_else(Fail::internal)?
        ));
    }
    if video.status != "RECORDING" {
        body.push_str("#EXT-X-ENDLIST\n");
    }
    Ok((
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body,
    )
        .into_response())
}
async fn segment(
    State(app): State<App>,
    jar: CookieJar,
    Path((id, name)): Path<(String, String)>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (video, ticket) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    if !matches!(ticket.scope.as_str(), "play" | "review")
        || (!video.recording && ticket.scope != "review")
        || name.len() > 80
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err(Fail::missing());
    }
    let key = format!("{id}/{name}");
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM video_segments s JOIN video_objects o ON o.key=s.object_key WHERE s.video_id=$1 AND s.object_key=$2 AND o.ready)").bind(&id).bind(&key).fetch_one(&app.db).await?;
    if !exists {
        return Err(Fail::missing());
    }
    let bytes = app
        .config
        .videos
        .storage
        .get(&app.http, &key)
        .await?
        .ok_or_else(Fail::missing)?;
    Ok(([(header::CONTENT_TYPE, "video/mp2t")], bytes).into_response())
}
async fn thumbnail(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (video, _) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    let key = video.thumbnail_key.as_deref().ok_or_else(Fail::missing)?;
    let bytes = app
        .config
        .videos
        .storage
        .get(&app.http, key)
        .await?
        .ok_or_else(Fail::missing)?;
    Ok(([(header::CONTENT_TYPE, "image/webp")], bytes).into_response())
}
fn range(value: Option<&str>, length: u64) -> Res<(u64, u64, bool)> {
    if length == 0 {
        return Err(Fail::missing());
    }
    let bad = || {
        Fail::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Choose one valid byte range.",
        )
    };
    let Some(value) = value else {
        return Ok((0, length - 1, false));
    };
    let (start, end) = value
        .strip_prefix("bytes=")
        .and_then(|v| v.split_once('-'))
        .ok_or_else(bad)?;
    let (start, end) = if start.is_empty() {
        let tail: u64 = end.parse().map_err(|_| bad())?;
        if tail == 0 {
            return Err(bad());
        }
        (length.saturating_sub(tail), length - 1)
    } else {
        let start: u64 = start.parse().map_err(|_| bad())?;
        let end = if end.is_empty() {
            length - 1
        } else {
            end.parse::<u64>().map_err(|_| bad())?.min(length - 1)
        };
        (start, end)
    };
    if start >= length || end < start {
        return Err(bad());
    }
    Ok((start, end, true))
}
async fn file(
    State(app): State<App>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(link): Query<Link>,
) -> Res<Response> {
    let (video, ticket) = verify_ticket(&app, &jar, &id, &link.ticket).await?;
    let download = ticket.scope == "download";
    let key = if download {
        if video.download_until.is_none_or(|at| at <= Utc::now()) {
            return Err(Fail::missing());
        }
        video.download_key
    } else {
        if video.kind != "CLIP" {
            return Err(Fail::denied(
                "Only the streamer and editors can download recordings.",
            ));
        }
        video.mp4_key
    }
    .ok_or_else(Fail::missing)?;
    let bytes: Option<i64> = sqlx::query_scalar(
        "SELECT bytes FROM video_objects WHERE video_id=$1 AND key=$2 AND ready",
    )
    .bind(&id)
    .bind(&key)
    .fetch_optional(&app.db)
    .await?;
    let length = bytes.ok_or_else(Fail::missing)? as u64;
    let (start, end, partial) = range(
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
    if download {
        response = response.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"sver-{id}.mp4\""),
        );
    }
    response
        .body(Body::from_stream(stream))
        .map_err(|_| Fail::internal())
}
async fn download(
    State(app): State<App>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Res<Json<Value>> {
    let user = profiles::signed_in(&app, &jar).await?;
    let video = load(&mut *app.db.acquire().await?, &id).await?;
    downloadable(&app, &video, &user).await?;
    if video.download_key.is_some() && video.download_until.is_some_and(|t| t > Utc::now()) {
        let token = ticket(&app, &video, Some(&user), "download", true)?;
        return Ok(Json(
            json!({"url":format!("/api/videos/{id}/file?ticket={token}")}),
        ));
    }
    enqueue(
        &mut *app.db.acquire().await?,
        &id,
        "DOWNLOAD",
        Some(&format!("{id}:download")),
        json!({}),
    )
    .await?;
    Ok(Json(json!({"queued":true})))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/videos/{id}/playlist", get(playlist))
        .route("/api/videos/{id}/segments/{name}", get(segment))
        .route("/api/videos/{id}/thumbnail", get(thumbnail))
        .route("/api/videos/{id}/file", get(file))
        .route("/api/videos/{id}/download", post(download))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeks_never_cross_the_object_or_accept_multiple_ranges() {
        assert_eq!(range(Some("bytes=3-8"), 10).unwrap(), (3, 8, true));
        assert_eq!(range(Some("bytes=-3"), 10).unwrap(), (7, 9, true));
        assert_eq!(range(Some("bytes=3-"), 10).unwrap(), (3, 9, true));
        for value in ["bytes=10-", "bytes=8-3", "bytes=1-2,4-5", "bytes=-0"] {
            assert!(range(Some(value), 10).is_err());
        }
    }
}
