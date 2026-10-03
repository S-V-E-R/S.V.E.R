//! Chat over HTTP and a real WebSocket: permissions, validation, idempotency, rate limits,
//! block filtering and live fanout.
use super::Env;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use sver::security as sec;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tower::ServiceExt;

async fn person(e: &Env, id: &str, name: &str, verified: bool) -> String {
    e.sql(&format!("INSERT INTO users(id,email,username,email_verified,date_of_birth) VALUES('{id}','{id}@example.test','{name}',{verified},'1990-01-01')")).await;
    let token = sec::token();
    sqlx::query("INSERT INTO sessions(id,user_id,token_hash,auth_version,mfa_verified,user_agent) SELECT $1,id,$2,auth_version,false,'synthetic' FROM users WHERE id=$3")
        .bind(format!("{id}-session")).bind(sec::digest(&token)).bind(id).execute(&e.app.db).await.unwrap();
    token
}
async fn call(
    e: &Env,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("origin", &e.app.config.origin)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    if let Some(token) = token {
        request = request.header("cookie", format!("sver_dev={token}"));
    }
    let response = sver::router(e.app.clone())
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
async fn say(e: &Env, token: &str, body: &str) -> (StatusCode, Value) {
    call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(token),
        json!({"id":id(),"body":body}),
    )
    .await
}
fn bodies(history: &Value) -> Vec<String> {
    history["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["body"].as_str().unwrap().to_string())
        .collect()
}
async fn next_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("socket event")
        .unwrap()
        .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

pub async fn exercise(e: &Env) {
    let chatter = person(e, "chat-user", "Chatter", true).await;
    let unverified = person(e, "chat-unverified", "Unverified", false).await;
    let watcher = person(e, "chat-watcher", "Watcher", true).await;

    let (status, history) = call(e, "GET", "/api/channels/streamer/chat", None, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["messages"], json!([]));
    assert_eq!(
        call(
            e,
            "GET",
            "/api/channels/nobody-here/chat",
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/streamer/chat",
            None,
            json!({"id":id(),"body":"hi"})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, denied) = say(e, &unverified, "hi").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(denied.to_string().contains("Verify your email"));

    // Validation: length, line breaks, control characters and the message ID.
    assert_eq!(
        say(e, &chatter, &"x".repeat(501)).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(say(e, &chatter, "   ").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        say(e, &chatter, "a\nb\nc\nd\ne\nf").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        say(e, &chatter, "bell\u{7}").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/streamer/chat",
            Some(&chatter),
            json!({"id":"not-a-uuid","body":"hi"})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // A retried send with the same ID returns the same message and stores it once.
    let message_id = id();
    let first = call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(&chatter),
        json!({"id":message_id,"body":"  <b>hello</b>  "}),
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(
        first.1["message"]["body"], "<b>hello</b>",
        "stored as plain text, trimmed"
    );
    assert_eq!(first.1["message"]["author"]["username"], "Chatter");
    assert!(first.1["message"]["author"].get("email").is_none());
    let retry = call(
        e,
        "POST",
        "/api/channels/streamer/chat",
        Some(&chatter),
        json!({"id":message_id,"body":"  <b>hello</b>  "}),
    )
    .await;
    assert_eq!(retry.1["message"]["seq"], first.1["message"]["seq"]);
    // Another author cannot reuse that ID.
    assert_eq!(
        call(
            e,
            "POST",
            "/api/channels/streamer/chat",
            Some(&watcher),
            json!({"id":message_id,"body":"mine"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    // Two sends per second per account.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(say(e, &chatter, &"y".repeat(500)).await.0, StatusCode::OK);
    assert_eq!(say(e, &chatter, "two").await.0, StatusCode::OK);
    assert_eq!(
        say(e, &chatter, "three").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );

    // Blocks: the watcher hides the chatter in their own view only; an owner block stops sending.
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('chat-watcher','chat-user')")
        .await;
    let (_, watcher_view) = call(
        e,
        "GET",
        "/api/channels/streamer/chat",
        Some(&watcher),
        Value::Null,
    )
    .await;
    assert!(
        bodies(&watcher_view).is_empty(),
        "blocked author hidden for the blocker"
    );
    let (_, guest_view) = call(e, "GET", "/api/channels/streamer/chat", None, Value::Null).await;
    assert_eq!(bodies(&guest_view).len(), 3);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    e.sql("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES('stream-owner','chat-user')")
        .await;
    assert_eq!(
        say(e, &chatter, "after block").await.0,
        StatusCode::FORBIDDEN
    );
    e.sql("DELETE FROM user_blocks WHERE blocker_id='stream-owner'")
        .await;

    // The owner's messages carry the owner role.
    let (_, own) = e
        .request(
            "POST",
            "/api/channels/streamer/chat",
            json!({"id":id(),"body":"welcome"}),
            true,
            true,
            false,
        )
        .await;
    assert_eq!(own["message"]["role"], "owner");

    // WebSocket: origin check, snapshot, live fanout, sends over the socket, block filtering.
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
    let connect = |token: Option<String>, origin: String| async move {
        let mut request = format!("ws://{address}/api/chat/ws?channel=streamer")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        if let Some(token) = token {
            request
                .headers_mut()
                .insert("cookie", format!("sver_dev={token}").parse().unwrap());
        }
        tokio_tungstenite::connect_async(request).await
    };
    match connect(None, "https://evil.example".into()).await {
        Err(tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN)
        }
        other => panic!(
            "foreign origin must be refused, got {:?}",
            other.map(|_| ())
        ),
    }
    let origin = e.app.config.origin.clone();
    let (mut guest, _) = connect(None, origin.clone()).await.unwrap();
    let snapshot = next_json(&mut guest).await;
    assert_eq!(snapshot["type"], "snapshot");
    assert_eq!(snapshot["messages"].as_array().unwrap().len(), 4);
    let (mut blocker, _) = connect(Some(watcher.clone()), origin.clone())
        .await
        .unwrap();
    assert_eq!(
        next_json(&mut blocker).await["messages"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "only the owner's message"
    );
    let (mut sender, _) = connect(Some(chatter.clone()), origin).await.unwrap();
    next_json(&mut sender).await;

    let socket_id = id();
    sender
        .send(tungstenite::Message::Text(
            json!({"id":socket_id,"body":"over the socket"})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let mut acked = false;
    for _ in 0..2 {
        let event = next_json(&mut sender).await;
        if event["type"] == "ack" {
            assert_eq!(event["id"], socket_id);
            acked = true;
        }
    }
    assert!(acked, "sender receives an ack");
    let live = next_json(&mut guest).await;
    assert_eq!(
        (live["type"].as_str(), live["message"]["body"].as_str()),
        (Some("message"), Some("over the socket"))
    );
    // The blocker does not receive it; the next thing they see is the owner's next message.
    e.request(
        "POST",
        "/api/channels/streamer/chat",
        json!({"id":id(),"body":"owner again"}),
        true,
        true,
        false,
    )
    .await;
    assert_eq!(
        next_json(&mut blocker).await["message"]["body"],
        "owner again"
    );
    sender
        .send(tungstenite::Message::Text("not json".into()))
        .await
        .unwrap();
    // The owner's broadcast may arrive first; the malformed command still gets an error reply.
    let mut errored = false;
    for _ in 0..2 {
        errored |= next_json(&mut sender).await["type"] == "error";
    }
    assert!(errored, "malformed command is answered with an error");

    server.abort();
    e.sql("DELETE FROM user_blocks").await;
    e.sql("DELETE FROM chat_messages").await;
    // Later lifecycle checks update every user row, so leave only the stream owner behind.
    e.sql("DELETE FROM users WHERE id LIKE 'chat-%'").await;
}
