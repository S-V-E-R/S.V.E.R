//! Live events (docs/DEVELOPER_PLATFORM.md §2): public topics with a client ID, private ones only
//! with the owner's `events:private` token, fan-out from the outbox, replay after an ID, ping.
use super::chat::{call, next_json, person};
use super::*;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

pub async fn exercise(e: &Env) {
    let created = e
        .call(
            "POST",
            "/api/me/apps",
            json!({"name":"Alerts Overlay","redirect_uris":["https://example.test/cb"]}),
        )
        .await;
    let client = created["created"]["client_id"]
        .as_str()
        .unwrap()
        .to_string();
    // An owner's token with events:private (as the OAuth flow would issue).
    e.sql("INSERT INTO oauth_grants(id,app_id,user_id,scopes) SELECT 'ev-grant',id,'stream-owner',ARRAY['events:private'] FROM dev_apps WHERE name='Alerts Overlay'").await;
    sqlx::query("INSERT INTO oauth_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,'ev-grant','access',now()+interval '1 hour')")
        .bind(sver::security::digest("ev-owner-token")).execute(&e.app.db).await.unwrap();
    sver::events::drain(&e.app).await.unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = sver::router(e.app.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    let connect = |query: String| async move {
        tokio_tungstenite::connect_async(
            format!("ws://{address}/api/events?{query}")
                .into_client_request()
                .unwrap(),
        )
        .await
    };
    match connect("client_id=sv_nobody".into()).await {
        Err(tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED)
        }
        other => panic!("an unknown app is refused, got {:?}", other.map(|_| ())),
    }
    let send = |text: Value| tungstenite::Message::Text(text.to_string().into());
    let (mut public, _) = connect(format!("client_id={client}")).await.unwrap();
    public.send(send(json!({"type":"method","id":1,"method":"subscribe","params":{"topics":["channel:Streamer:follows","channel:streamer:subs","channel:streamer:nonsense"]}}))).await.unwrap();
    let reply = next_json(&mut public).await;
    assert_eq!(
        (&reply["id"], &reply["result"]["subscribed"]),
        (&json!(1), &json!(["channel:streamer:follows"])),
        "{reply}"
    );
    assert_eq!(
        reply["result"]["refused"].as_array().unwrap().len(),
        2,
        "private needs the owner's token"
    );
    let (mut owner, _) = connect(format!("client_id={client}&access_token=ev-owner-token"))
        .await
        .unwrap();
    owner.send(send(json!({"type":"method","id":"a","method":"subscribe","params":{"topics":["channel:streamer:follows:detail"]}}))).await.unwrap();
    assert_eq!(
        next_json(&mut owner).await["result"]["subscribed"],
        json!(["channel:streamer:follows:detail"])
    );

    // A follow: the count is public, the follower's name only on the private topic.
    let fan = person(e, "ev-fan", "EvFan", true).await;
    call(e, "PUT", "/api/follows/Streamer", Some(&fan), Value::Null).await;
    sver::events::drain(&e.app).await.unwrap();
    let count = next_json(&mut public).await;
    assert_eq!(count["topic"], "channel:streamer:follows");
    assert!(count["data"]["followers"].as_i64().unwrap() >= 1);
    assert!(
        !count.to_string().contains("EvFan"),
        "public events don't name followers"
    );
    let detail = next_json(&mut owner).await;
    assert_eq!(detail["data"]["user"], "EvFan");

    // Replay after an ID, and ping.
    let since = count["id"].as_i64().unwrap() - 1;
    let (mut late, _) = connect(format!("client_id={client}")).await.unwrap();
    late.send(send(json!({"type":"method","id":2,"method":"subscribe","params":{"topics":["channel:streamer:follows"],"since":since}}))).await.unwrap();
    next_json(&mut late).await;
    assert_eq!(
        next_json(&mut late).await["id"],
        count["id"],
        "missed events are replayed"
    );
    late.send(send(json!({"type":"method","id":3,"method":"ping"})))
        .await
        .unwrap();
    assert_eq!(next_json(&mut late).await["result"], "pong");
    server.abort();
    for statement in [
        "DELETE FROM dev_apps",
        "DELETE FROM follows WHERE follower_id='ev-fan'",
    ] {
        e.sql(statement).await;
    }
}
