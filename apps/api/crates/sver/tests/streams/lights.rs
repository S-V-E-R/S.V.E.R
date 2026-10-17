//! Lights and Shine Moments (docs/SUPPORT.md "Shine"): free, limited lights with checked messages,
//! hidden by the recipient; Moments limited to the channel's own public clips.
use super::Env;
use super::chat::{call, person};
use axum::http::StatusCode;
use serde_json::{Value, json};

pub async fn exercise(e: &Env) {
    let fan = person(e, "li-fan", "LightFan", true).await;
    for (id, name) in [
        ("li-one", "LightOne"),
        ("li-two", "LightTwo"),
        ("li-three", "LightThree"),
    ] {
        person(e, id, name, true).await;
    }
    let light = |to: &str, message: &str| {
        let body = json!({"to": to, "message": message});
        let token = fan.clone();
        async move { call(e, "POST", "/api/lights", Some(&token), body).await.0 }
    };
    assert_eq!(
        light("LightFan", "").await,
        StatusCode::BAD_REQUEST,
        "not yourself"
    );
    assert_eq!(
        light("Streamer", "see example.com").await,
        StatusCode::BAD_REQUEST,
        "no links"
    );
    assert_eq!(
        light("Streamer", "Great stream, thank you").await,
        StatusCode::OK
    );
    assert_eq!(
        light("Streamer", "again").await,
        StatusCode::CONFLICT,
        "once per person per day"
    );
    assert_eq!(light("LightOne", "").await, StatusCode::OK);
    assert_eq!(light("LightTwo", "").await, StatusCode::OK);
    assert_eq!(
        light("LightThree", "").await,
        StatusCode::CONFLICT,
        "3 a day"
    );
    let (_, got) = call(e, "GET", "/api/channels/Streamer/lights", None, Value::Null).await;
    assert_eq!(got["count"], 1);
    assert_eq!(got["recent"][0]["message"], "Great stream, thank you");
    let noticed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM notifications WHERE user_id='stream-owner' AND kind='light')",
    )
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(noticed);
    let id = got["recent"][0]["id"].as_str().unwrap().to_string();
    e.call("DELETE", &format!("/api/me/lights/{id}"), Value::Null)
        .await;
    assert_eq!(
        call(e, "GET", "/api/channels/Streamer/lights", None, Value::Null)
            .await
            .1["count"],
        0
    );

    // Parallel gives can't beat the daily limit.
    let racer = person(e, "li-racer", "LightRacer", true).await;
    for n in 0..5 {
        person(e, &format!("li-r{n}"), &format!("LightRecv{n}"), true).await;
    }
    let gives = (0..5).map(|n| {
        let token = racer.clone();
        async move {
            call(
                e,
                "POST",
                "/api/lights",
                Some(&token),
                json!({"to": format!("LightRecv{n}")}),
            )
            .await
            .0
        }
    });
    let results = futures_util::future::join_all(gives).await;
    assert_eq!(
        results.iter().filter(|s| **s == StatusCode::OK).count(),
        3,
        "{results:?}"
    );
    let given: i64 = sqlx::query_scalar("SELECT count(*) FROM lights WHERE from_id='li-racer'")
        .fetch_one(&e.app.db)
        .await
        .unwrap();
    assert_eq!(given, 3);

    // Shine Moments: only the channel's own public, ready clips.
    let clip = uuid::Uuid::new_v4().to_string();
    let private = uuid::Uuid::new_v4().to_string();
    for (id, visibility) in [(&clip, "PUBLIC"), (&private, "PRIVATE")] {
        sqlx::query("INSERT INTO videos(id,owner_id,kind,status,approval,visibility,title,started_at) VALUES($1,'stream-owner','CLIP','READY','APPROVED',$2,'Synthetic moment',now())")
            .bind(id).bind(visibility).execute(&e.app.db).await.unwrap();
    }
    let (status, _) = e
        .request(
            "PUT",
            "/api/me/moments",
            json!({"items": [{"video_id": private}]}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "private clips can't be pinned"
    );
    let pinned = e
        .call(
            "PUT",
            "/api/me/moments",
            json!({"items": [{"video_id": clip, "title": "The comeback"}]}),
        )
        .await;
    assert_eq!(pinned["moments"][0]["title"], "The comeback");
    // A clip made private later leaves the profile on its own.
    sqlx::query("UPDATE videos SET visibility='PRIVATE' WHERE id=$1")
        .bind(&clip)
        .execute(&e.app.db)
        .await
        .unwrap();
    assert_eq!(
        call(
            e,
            "GET",
            "/api/channels/Streamer/moments",
            None,
            Value::Null
        )
        .await
        .1["moments"],
        json!([])
    );
    sqlx::query("DELETE FROM videos WHERE id=ANY($1)")
        .bind([&clip, &private])
        .execute(&e.app.db)
        .await
        .unwrap();
}
