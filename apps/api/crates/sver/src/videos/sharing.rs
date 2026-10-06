use super::*;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};

async fn shared(app: &App, id: &str) -> Res<(Video, Value)> {
    let video = load(&mut *app.db.acquire().await?, id).await?;
    if video.kind != "CLIP"
        || video.visibility != "PUBLIC"
        || video.mature
        || video.status != "READY"
        || video.approval != "APPROVED"
    {
        return Err(Fail::missing());
    }
    accessible(app, &video, None, false).await?;
    let owner = profiles::channel_user_by_id(
        &mut *app.db.acquire().await?,
        video.owner_id.as_deref().ok_or_else(Fail::missing)?,
    )
    .await?
    .ok_or_else(Fail::missing)?;
    let token = ticket(app, &video, None, "play", false)?;
    let origin = &app.config.origin;
    let data = json!({"id":id,"title":video.title,"author_name":owner.display_name,"author_url":format!("{origin}/{}",owner.username),"url":format!("{origin}/clips/{id}"),"embed":format!("{origin}/embed/{id}"),"mp4":format!("{origin}/api/videos/{id}/file?ticket={token}"),"thumbnail":video.thumbnail_key.as_ref().map(|_|format!("{origin}/api/videos/{id}/thumbnail?ticket={token}"))});
    Ok((video, data))
}
async fn share(State(app): State<App>, Path(id): Path<String>) -> Res<Json<Value>> {
    Ok(Json(shared(&app, &id).await?.1))
}
#[derive(Deserialize)]
struct Embed {
    url: String,
    maxwidth: Option<u32>,
    maxheight: Option<u32>,
    format: Option<String>,
}
async fn oembed(State(app): State<App>, Query(input): Query<Embed>) -> Res<Json<Value>> {
    if input.format.as_deref().is_some_and(|s| s != "json") {
        return Err(Fail::bad("Only JSON oEmbed is supported."));
    }
    let url = url::Url::parse(&input.url).map_err(|_| Fail::bad("Use a S.V.E.R clip link."))?;
    let origin = url::Url::parse(&app.config.origin).map_err(|_| Fail::internal())?;
    let id = url
        .path()
        .strip_prefix("/clips/")
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(Fail::missing)?;
    if url.origin() != origin.origin() || !url.username().is_empty() || url.password().is_some() {
        return Err(Fail::bad("Use a S.V.E.R clip link."));
    }
    let (_, data) = shared(&app, id).await?;
    let width = input
        .maxwidth
        .unwrap_or(640)
        .clamp(160, 1920)
        .min(input.maxheight.unwrap_or(1080).clamp(90, 1080) * 16 / 9);
    let height = width * 9 / 16;
    Ok(Json(
        json!({"version":"1.0","type":"video","provider_name":"S.V.E.R","provider_url":app.config.origin,"title":data["title"],"author_name":data["author_name"],"author_url":data["author_url"],"thumbnail_url":data["thumbnail"],"width":width,"height":height,"html":format!("<iframe src=\"{}/embed/{id}\" width=\"{width}\" height=\"{height}\" title=\"S.V.E.R clip\" allow=\"fullscreen\" allowfullscreen></iframe>",app.config.origin)}),
    ))
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/videos/{id}/share", get(share))
        .route("/api/oembed", get(oembed))
}
