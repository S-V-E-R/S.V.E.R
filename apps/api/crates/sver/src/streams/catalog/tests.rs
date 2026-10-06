use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn game(id: &str, name: &str, genre: &str, aliases: &str) -> Value {
    json!({"game":{"value":format!("http://www.wikidata.org/entity/{id}")},"name":{"value":name},"genres":{"value":genre},"aliases":{"value":aliases}})
}
#[test]
fn classifies_explicit_genres_and_rejects_ambiguous_prose() {
    for (source, expected) in [
        ("first-person shooter", "fps_battle_royale"),
        (
            "massively multiplayer online role-playing game",
            "mmos_rpgs",
        ),
        ("third-person shooter", "fps_battle_royale"),
        ("racing video game", "sports_racing"),
    ] {
        assert_eq!(classify(&[source.into()], ""), Some(expected));
    }
    assert_eq!(
        classify(
            &[
                "action-adventure game".into(),
                "third-person shooter".into()
            ],
            ""
        ),
        Some("fps_battle_royale")
    );
    assert_eq!(
        classify(
            &[
                "first-person shooter".into(),
                "role-playing video game".into()
            ],
            ""
        ),
        None
    );
    assert_eq!(
        classify(&[], "2026 first-person shooter video game"),
        Some("fps_battle_royale")
    );
    assert_eq!(
        classify(
            &[],
            "A puzzle game with elements inspired by a first-person shooter"
        ),
        None
    );
    assert_eq!(classify(&[], "Not a first-person shooter"), None);
    assert_eq!(classify(&[], "The Shooter's Apprentice"), None);
    assert!(query("Q12\" } UNION {", true).is_err());
    assert!(source_id("https://example.test/Q123").is_err());
    assert!(!valid_id("Q0"));
}

async fn choices(app: &App, query: &str) -> Value {
    let response = crate::router(app.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/categories?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn refresh_preserves_local_choices_through_outages_and_staff_changes() {
    let url = std::env::var("DATABASE_URL").expect("use scripts/dev.ps1 test");
    let parsed = url::Url::parse(&url).unwrap();
    assert!(
        matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"))
            && parsed.path() == "/sver_rebuild"
    );
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("catalog_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let statement = format!("SET search_path TO {schema}");
    let db = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let statement = statement.clone();
            Box::pin(async move {
                sqlx::query(sqlx::AssertSqlSafe(statement))
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("../../../../migrations")
        .run(&db)
        .await
        .unwrap();
    let app = App::new(db.clone(), crate::Config::from_env().unwrap())
        .await
        .unwrap();
    let response = Arc::new(Mutex::new((
        StatusCode::OK,
        json!({"results":{"bindings":[
            game("Q100","Synthetic Frontier","first-person shooter","SF 4|Frontier IV"),
            game("Q101","Synthetic Kingdom","massively multiplayer online role-playing game",""),
            game("Q102","Synthetic Hybrid","first-person shooter|role-playing video game",""),
            game("Q103","Minecraft","sandbox game","")
        ]}}),
    )));
    let state = response.clone();
    let router = Router::new().route(
        "/sparql",
        get(move || {
            let (status, body) = state.lock().unwrap().clone();
            async move { (status, [("retry-after", "600")], Json(body)) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/sparql", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let result=tokio::spawn(async move {
        tick_at(&app,&endpoint).await.unwrap();
        let matches=choices(&app,"q=SF%204").await;
        assert_eq!(matches["categories"][0]["name"],"Synthetic Frontier");
        assert_eq!(choices(&app,"q=Hybrid").await["categories"],json!([]));
        let total:i64=sqlx::query_scalar("SELECT count(*) FROM stream_categories").fetch_one(&app.db).await.unwrap();
        assert_eq!(total,8,"unselected imports must not flood Browse");
        assert_eq!(choices(&app,"q=minecraft").await["categories"].as_array().unwrap().len(),1);
        let mut tx=app.db.begin().await.unwrap();
        let selected=select(&mut tx,"wikidata-q100").await.unwrap();
        assert_eq!(selected,"wikidata-q100");
        tx.commit().await.unwrap();
        let genre:String=sqlx::query_scalar("SELECT genre FROM stream_categories WHERE id='wikidata-q100'").fetch_one(&app.db).await.unwrap();
        assert_eq!(genre,"fps_battle_royale");
        // Staff edits win even after the upstream game changes its title and genre.
        sqlx::query("UPDATE stream_categories SET name='Staff title',genre='mmos_rpgs',active=false WHERE id='wikidata-q100'").execute(&app.db).await.unwrap();
        response.lock().unwrap().1=json!({"results":{"bindings":[game("Q100","Provider rename","racing video game","SF 4")]}});
        sqlx::query("UPDATE game_catalog_sync SET next_run_at=now()").execute(&app.db).await.unwrap();
        tick_at(&app,&endpoint).await.unwrap();
        let row:(String,String,bool)=sqlx::query_as("SELECT name,genre,active FROM stream_categories WHERE id='wikidata-q100'").fetch_one(&app.db).await.unwrap();
        assert_eq!(row,("Staff title".into(),"mmos_rpgs".into(),false));
        assert_eq!(choices(&app,"q=SF%204").await["categories"],json!([]),"hidden imports never reappear");
        *response.lock().unwrap()=(StatusCode::SERVICE_UNAVAILABLE,json!({"error":"offline"}));
        sqlx::query("UPDATE game_catalog_sync SET next_run_at=now()").execute(&app.db).await.unwrap();
        assert!(tick_at(&app,&endpoint).await.is_err());
        assert_eq!(choices(&app,"q=Kingdom").await["categories"][0]["name"],"Synthetic Kingdom");
        let mut tx=app.db.begin().await.unwrap();
        assert_eq!(select(&mut tx,"wikidata-q101").await.unwrap(),"wikidata-q101");
        assert!(select(&mut tx,"wikidata-q999").await.is_err());
        assert!(select(&mut tx,"wikidata-q102").await.is_err());
        tx.commit().await.unwrap();
        let (failures,scheduled):(i32,bool)=sqlx::query_as("SELECT failures,next_run_at>now()+interval '9 minutes' FROM game_catalog_sync").fetch_one(&app.db).await.unwrap();
        assert_eq!((failures,scheduled),(1,true));
        // The lease prevents duplicate workers from issuing a concurrent refresh.
        sqlx::query("UPDATE game_catalog_sync SET next_run_at=now(),lease_until=now()+interval '1 minute',lease_token='other-worker'").execute(&app.db).await.unwrap();
        tick_at(&app,&endpoint).await.unwrap();
        // Malformed response cannot advance the cursor or delete saved data.
        *response.lock().unwrap()=(StatusCode::OK,json!({"results":{"bindings":[game("Q102","Z","racing game",""),game("Q101","A","racing game","")]}}));
        sqlx::query("UPDATE game_catalog_sync SET lease_until=NULL,next_run_at=now()").execute(&app.db).await.unwrap();
        assert!(tick_at(&app,&endpoint).await.is_err());
        let cursor:String=sqlx::query_scalar("SELECT cursor FROM game_catalog_sync").fetch_one(&app.db).await.unwrap();
        assert_eq!(cursor,"");
    }).await;
    server.abort();
    db.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    result.unwrap();
}
