//! Audit regressions for follows, blocks, War Council and onboarding suggestions (social.rs).
use super::*;

async fn restrict(e: &Env, id: &str) {
    sqlx::query("INSERT INTO profiles(user_id,display_name,restricted_until) VALUES($1,'Synthetic',now()+interval '1 day') ON CONFLICT (user_id) DO UPDATE SET restricted_until=EXCLUDED.restricted_until")
        .bind(id)
        .execute(&e.app.db)
        .await
        .unwrap();
}
async fn spend_block_budget(e: &Env, id: &str) {
    sqlx::query("INSERT INTO rate_limits(key,count,expires_at) VALUES($1,30,now()+interval '1 hour') ON CONFLICT (key) DO UPDATE SET count=30,expires_at=EXCLUDED.expires_at")
        .bind(format!("block:{id}"))
        .execute(&e.app.db)
        .await
        .unwrap();
}
async fn blocks(e: &Env, blocker: &str, blocked: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id=$1 AND blocked_id=$2)",
    )
    .bind(blocker)
    .bind(blocked)
    .fetch_one(&e.app.db)
    .await
    .unwrap()
}

pub async fn exercise(e: &Env) {
    e.reset_limits().await;
    let (a_id, a) = e.user("AsAlice", true).await;
    let (b_id, _) = e.user("AsBob", true).await;

    // Unfollowing a hidden channel answers exactly like a name that was never registered.
    a.ok("PUT", "/api/follows/AsBob", Value::Null).await;
    restrict(e, &b_id).await;
    let hidden = a.call("DELETE", "/api/follows/AsBob", Value::Null).await;
    let unknown = a
        .call("DELETE", "/api/follows/AsNeverRegistered", Value::Null)
        .await;
    assert_eq!(hidden.0, StatusCode::NOT_FOUND);
    assert_eq!(hidden, unknown);
    assert!(hidden.1.get("follower_count").is_none());

    // Blocking answers the same for restricted, internal, deleted and unknown names.
    let (internal_id, _) = e.user("AsInternal", true).await;
    sqlx::query("INSERT INTO profiles(user_id,display_name,internal) VALUES($1,'Synthetic',true) ON CONFLICT (user_id) DO UPDATE SET internal=true")
        .bind(&internal_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    let (deleted_id, _) = e.user("AsDeleted", true).await;
    sqlx::query("UPDATE users SET deleted_at=now() WHERE id=$1")
        .bind(&deleted_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    for name in ["AsBob", "AsInternal", "AsDeleted", "AsNobodyHere"] {
        assert_eq!(
            a.call("PUT", &format!("/api/blocks/{name}"), Value::Null)
                .await,
            (StatusCode::OK, json!({"blocked": true})),
            "{name}"
        );
    }
    assert!(blocks(e, &a_id, &b_id).await);
    assert!(!blocks(e, &a_id, &internal_id).await);
    assert!(!blocks(e, &a_id, &deleted_id).await);

    // An unblock over the hourly limit is refused and the block stays.
    spend_block_budget(e, &a_id).await;
    assert_eq!(
        a.status("DELETE", "/api/blocks/AsBob", Value::Null).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(blocks(e, &a_id, &b_id).await);
    e.reset_limits().await;
    a.ok("DELETE", "/api/blocks/AsBob", Value::Null).await;
    assert!(!blocks(e, &a_id, &b_id).await);

    // Blocking from the wall queue spends the same budget as PUT /api/blocks.
    let (owner_id, owner) = e.user("AsWallOwner", true).await;
    let (author_id, _) = e.user("AsWallAuthor", true).await;
    sqlx::query("INSERT INTO wall_posts(id,wall_owner_id,author_id,body,status) VALUES('as-wall-post',$1,$2,'Synthetic','PENDING')")
        .bind(&owner_id)
        .bind(&author_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    spend_block_budget(e, &owner_id).await;
    assert_eq!(
        owner
            .status(
                "POST",
                "/api/wall/posts/as-wall-post/block-author",
                Value::Null
            )
            .await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert!(!blocks(e, &owner_id, &author_id).await);
    e.reset_limits().await;
    owner
        .ok(
            "POST",
            "/api/wall/posts/as-wall-post/block-author",
            Value::Null,
        )
        .await;
    assert!(blocks(e, &owner_id, &author_id).await);

    // A third party's War Council leaves out members in a block with the viewer, either way.
    let (_, council) = e.user("AsCouncilOwner", true).await;
    e.user("AsCouncilKept", true).await;
    let (_, member) = e.user("AsCouncilBlocked", true).await;
    let (_, viewer) = e.user("AsCouncilViewer", true).await;
    let (_, other) = e.user("AsCouncilOther", true).await;
    council
        .ok(
            "PUT",
            "/api/me/war-council",
            json!({"members": ["AsCouncilBlocked", "AsCouncilKept"]}),
        )
        .await;
    viewer
        .ok("PUT", "/api/blocks/AsCouncilBlocked", Value::Null)
        .await;
    member
        .ok("PUT", "/api/blocks/AsCouncilOther", Value::Null)
        .await;
    let names = |page: Value| -> Vec<(String, bool)> {
        page["war_council"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["user"]["username"].as_str().unwrap().to_string(),
                    m["crown"].as_bool().unwrap(),
                )
            })
            .collect()
    };
    for client in [&viewer, &other] {
        assert_eq!(
            names(
                client
                    .ok("GET", "/api/channels/AsCouncilOwner", Value::Null)
                    .await
            ),
            [("AsCouncilKept".to_string(), true)]
        );
    }
    assert_eq!(
        names(
            e.anon
                .ok("GET", "/api/channels/AsCouncilOwner", Value::Null)
                .await
        )
        .len(),
        2
    );

    // A block committed while a follow waits on the profile locks still stops the follow.
    let (race_a_id, race_a) = e.user("AsRaceFan", true).await;
    let (race_b_id, _) = e.user("AsRaceHost", true).await;
    for id in [&race_a_id, &race_b_id] {
        sqlx::query("INSERT INTO profiles(user_id,display_name) VALUES($1,'Synthetic') ON CONFLICT (user_id) DO NOTHING")
            .bind(id)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    // Hold the profile locks first, so the follow passes its first block check (no block yet) and
    // then waits; the block lands while it waits, and only the re-check under the locks sees it.
    let mut tx = e.app.db.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM profiles WHERE user_id IN ($1,$2) ORDER BY user_id FOR UPDATE")
        .bind(&race_a_id)
        .bind(&race_b_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let pending = tokio::spawn(async move {
        race_a
            .status("PUT", "/api/follows/AsRaceHost", Value::Null)
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !pending.is_finished(),
        "the follow waits on the profile locks"
    );
    sqlx::query("INSERT INTO user_blocks(blocker_id,blocked_id) VALUES($1,$2)")
        .bind(&race_b_id)
        .bind(&race_a_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::FORBIDDEN);
    let followed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM follows WHERE follower_id=$1 AND following_id=$2)",
    )
    .bind(&race_a_id)
    .bind(&race_b_id)
    .fetch_one(&e.app.db)
    .await
    .unwrap();
    assert!(!followed);

    // Follower pages: every follower exactly once, in order, across ties on a page boundary.
    let (paged_id, _) = e.user("AsPaged", true).await;
    sqlx::query("INSERT INTO users(id,email,username,email_verified,created_at,date_of_birth) SELECT 'as-fan-'||lpad(g::text,2,'0'),'as-fan-'||g||'@example.invalid','AsFan'||g,true,now()-interval '30 days','1995-01-01' FROM generate_series(1,60) g")
        .execute(&e.app.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO follows(follower_id,following_id,created_at,counted_at) SELECT 'as-fan-'||lpad(g::text,2,'0'),$1,timestamptz '2026-01-01 00:00:00+00'+(g/2)*interval '1 minute',now() FROM generate_series(1,60) g")
        .bind(&paged_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    let expected: Vec<String> = sqlx::query_scalar("SELECT u.username FROM follows f JOIN users u ON u.id=f.follower_id WHERE f.following_id=$1 ORDER BY f.created_at DESC, f.follower_id DESC")
        .bind(&paged_id)
        .fetch_all(&e.app.db)
        .await
        .unwrap();
    let mut seen = Vec::new();
    let mut path = "/api/channels/AsPaged/followers".to_string();
    loop {
        let page = e.anon.ok("GET", &path, Value::Null).await;
        for item in page["items"].as_array().unwrap() {
            seen.push(item["user"]["username"].as_str().unwrap().to_string());
        }
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("/api/channels/AsPaged/followers?cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(seen, expected);

    // Onboarding suggestions leave out mature-labeled live channels for under-18 accounts.
    let (mature_id, _) = e.user("AsMatureLive", true).await;
    let (plain_id, _) = e.user("AsPlainLive", true).await;
    sqlx::query("INSERT INTO stream_settings(owner_id,title,mature) VALUES($1,'Synthetic',true)")
        .bind(&mature_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    for (n, id) in [&mature_id, &plain_id].into_iter().enumerate() {
        sqlx::query("INSERT INTO broadcasts(id,owner_id,public_id,generation,state,server_id,service_id,client_id,started_at,publisher_started_at,startup_deadline,alert_state) VALUES($1,$2,$1,1,'LIVE','s','v','c',now()-interval '3 minutes',now(),now(),'skipped')")
            .bind(format!("as-live-{n}"))
            .bind(id)
            .execute(&e.app.db)
            .await
            .unwrap();
    }
    let (minor_id, minor) = e.user("AsMinor", true).await;
    sqlx::query("UPDATE users SET date_of_birth=current_date-interval '15 years' WHERE id=$1")
        .bind(&minor_id)
        .execute(&e.app.db)
        .await
        .unwrap();
    let (_, adult) = e.user("AsAdult", true).await;
    let suggested = |page: Value| -> Vec<String> {
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|c| c["username"].as_str().map(str::to_string))
            .collect()
    };
    let for_minor = suggested(minor.ok("GET", "/api/me/suggestions", Value::Null).await);
    assert!(
        !for_minor.contains(&"AsMatureLive".to_string()),
        "{for_minor:?}"
    );
    assert!(
        for_minor.contains(&"AsPlainLive".to_string()),
        "{for_minor:?}"
    );
    let for_adult = suggested(adult.ok("GET", "/api/me/suggestions", Value::Null).await);
    assert!(
        for_adult.contains(&"AsMatureLive".to_string()),
        "{for_adult:?}"
    );
    e.sql("UPDATE broadcasts SET state='ENDED',ended_at=now(),end_reason='test' WHERE id IN ('as-live-0','as-live-1')").await;
    e.reset_limits().await;
}
