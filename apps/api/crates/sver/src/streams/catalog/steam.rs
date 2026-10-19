//! Steam's store feed. Every Steam game with a store page, released or upcoming, enters the local
//! catalog as soon as Steam lists it. Games Wikidata also knows keep their Wikidata entry; the two
//! are linked by Steam app id. Like Wikidata, this never runs while a streamer saves or publishes.
use super::*;

const BASE: &str = "https://api.steampowered.com";
const BATCH: i64 = 200;

/// Steam tag ids for the war genres. Steam orders a game's tags by how many players applied them;
/// the first mapped tag among the top eight decides, so a shooter tagged "RPG" further down stays
/// a shooter. Broad tags (Action, Adventure, Indie, Casual) map to nothing.
fn tag_genre(tag: u64) -> Option<&'static str> {
    Some(match tag {
        // FPS, Battle Royale, Hero/Looter/Third-Person/Arena Shooter, Shooter
        1663 | 176981 | 620519 | 353880 | 3814 | 5547 | 1774 => "fps_battle_royale",
        // MMORPG, Massively Multiplayer, RPG, Action RPG, JRPG, CRPG, Party-Based RPG, Tactical RPG
        1754 | 128 | 122 | 4231 | 4434 | 4474 | 10695 | 21725 => "mmos_rpgs",
        // RTS, MOBA
        1676 | 1718 => "rts_moba",
        // Strategy, Turn-Based Strategy, 4X, Grand Strategy, Wargame, Auto Battler, Tower Defense
        9 | 1741 | 1670 | 4364 | 4684 | 1084988 | 1645 => "strategy_4x",
        // Fighting, 2D Fighter, 3D Fighter
        1743 | 4736 | 6506 => "fighting",
        // Racing, Sports, Driving, Automobile Sim, Football (Soccer), Basketball
        699 | 701 | 1644 | 1100687 | 1254546 | 1746 => "sports_racing",
        // Card Game, Board Game, Deckbuilding, Card Battler, Trading Card Game, Chess, Roguelike Deckbuilder
        1666 | 1770 | 32322 | 791774 | 9271 | 4184 | 1091588 => "card_board",
        // Puzzle, Simulation, Management, Match 3, Hidden Object, City Builder, Colony Sim
        1664 | 599 | 12472 | 1665 | 1738 | 4328 | 220585 => "puzzle_simulation",
        // Sandbox, Life Sim, Farming Sim, Cozy, Open World Survival Craft
        3810 | 10235 | 87918 | 97376 | 1100689 => "cozy_sandbox",
        // Party Game, Party
        7178 | 7108 => "coop_party",
        _ => return None,
    })
}
fn classify(tags: &[Tag]) -> Option<&'static str> {
    let mut tags: Vec<&Tag> = tags.iter().collect();
    tags.sort_by_key(|t| std::cmp::Reverse(t.weight));
    tags.iter().take(8).find_map(|t| tag_genre(t.tagid))
}

#[derive(Deserialize)]
struct AppList {
    response: AppPage,
}
#[derive(Deserialize, Default)]
struct AppPage {
    #[serde(default)]
    apps: Vec<SteamApp>,
    #[serde(default)]
    have_more_results: bool,
    #[serde(default)]
    last_appid: i64,
}
#[derive(Deserialize)]
struct SteamApp {
    appid: i64,
}
#[derive(Deserialize)]
struct Items {
    response: ItemPage,
}
#[derive(Deserialize, Default)]
struct ItemPage {
    #[serde(default)]
    store_items: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    #[serde(default)]
    appid: i64,
    #[serde(default)]
    success: i64,
    #[serde(default)]
    visible: bool,
    /// 0 is a game.
    #[serde(default, rename = "type")]
    kind: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    content_descriptorids: Vec<i64>,
    #[serde(default)]
    tags: Vec<Tag>,
}
#[derive(Deserialize)]
struct Tag {
    tagid: u64,
    #[serde(default)]
    weight: i64,
}

async fn import(db: &mut PgConnection, item: &Item) -> Res<()> {
    // Steam content descriptors: 3 adult-only sexual content, 4 frequent nudity or sexual content
    // are never offered; 1 some nudity, 2 frequent violence or gore, 5 general mature switch the
    // Mature label on.
    let descriptors = &item.content_descriptorids;
    if item.success != 1
        || !item.visible
        || item.kind != 0
        || !(1..10_000_000_000).contains(&item.appid)
        || descriptors.iter().any(|d| matches!(d, 3 | 4))
    {
        return Ok(());
    }
    let Some(genre) = classify(&item.tags) else {
        // ponytail: unclassifiable Steam games stay out instead of flooding the staff queue;
        // notable ones still arrive through Wikidata, which does queue them.
        return Ok(());
    };
    let Ok(name) = text::clean(&item.name, "name", false) else {
        return Ok(());
    };
    if name.is_empty() || name.chars().count() > 160 {
        return Ok(());
    }
    let id = format!("steam{}", item.appid);
    let mature = descriptors.iter().any(|d| matches!(d, 1 | 2 | 5));
    // A game another entry already covers (same Steam app or same title) keeps that entry.
    sqlx::query("INSERT INTO game_catalog(source_id,name,suggested_genre,mature,steam_app_id) SELECT $1,$2,$3,$4,$5 WHERE NOT EXISTS(SELECT 1 FROM game_catalog WHERE source_id<>$1 AND (steam_app_id=$5 OR lower(name)=lower($2))) ON CONFLICT(source_id) DO UPDATE SET name=$2,suggested_genre=$3,mature=$4,refreshed_at=now()")
        .bind(&id).bind(&name).bind(genre).bind(mature).bind(item.appid).execute(&mut *db).await?;
    // Link a staff-created category of the same title, as Wikidata imports do.
    sqlx::query("UPDATE game_catalog SET category_id=(SELECT id FROM stream_categories c WHERE lower(c.name)=lower($2) AND NOT EXISTS(SELECT 1 FROM game_catalog g WHERE g.category_id=c.id) ORDER BY id LIMIT 1) WHERE source_id=$1 AND category_id IS NULL AND NOT reviewed")
        .bind(&id).bind(&name).execute(db).await?;
    Ok(())
}

/// One page of changed apps per pass, with their store details. Off unless STEAM_WEB_API_KEY is set.
pub async fn tick(app: &App) -> Res<()> {
    match std::env::var("STEAM_WEB_API_KEY") {
        Ok(key) if !key.is_empty() => tick_at(app, BASE, &key).await,
        _ => Ok(()),
    }
}
pub(super) async fn tick_at(app: &App, base: &str, key: &str) -> Res<()> {
    // Claiming the run time is the lease: another worker waits five minutes or for this result.
    let claimed: Option<(i64, i64, i64)> = sqlx::query_as("UPDATE game_catalog_sync SET steam_next_run_at=now()+interval '5 minutes' WHERE steam_next_run_at<=now() RETURNING steam_cursor,steam_since,steam_pass")
        .fetch_optional(&app.db).await?;
    let Some((cursor, since, pass)) = claimed else {
        return Ok(());
    };
    // A pass asks for apps changed since the previous pass started, so nothing slips between them.
    let pass = if cursor == 0 {
        chrono::Utc::now().timestamp()
    } else {
        pass
    };
    match fetch(app, base, key, cursor, since).await {
        Ok((items, last, more)) => {
            let mut tx = app.db.begin().await?;
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext('stream-catalog'))")
                .execute(&mut *tx)
                .await?;
            for item in &items {
                import(&mut tx, item).await?;
            }
            let (cursor, since, wait) = if more {
                (last, since, 2.0)
            } else {
                (0, pass, 3600.0)
            };
            sqlx::query("UPDATE game_catalog_sync SET steam_cursor=$1,steam_since=$2,steam_pass=$3,steam_next_run_at=now()+make_interval(secs=>$4),steam_failures=0,steam_last_success_at=now()")
                .bind(cursor).bind(since).bind(pass).bind(wait).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(())
        }
        Err(retry) => {
            sqlx::query("UPDATE game_catalog_sync SET steam_next_run_at=now()+make_interval(secs=>$1),steam_failures=least(steam_failures+1,1000)")
                .bind(retry as f64).execute(&app.db).await?;
            Err(Fail::unavailable(
                "Steam catalog refresh will retry; saved games remain available.",
            ))
        }
    }
}

async fn get(app: &App, url: &str, query: &[(&str, String)]) -> Result<Vec<u8>, u64> {
    let response = app
        .http
        .get(url)
        .query(query)
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await
        .map_err(|_| 600u64)?;
    if !response.status().is_success() {
        return Err(retry_after(&response));
    }
    read(response, 4_000_000).await.ok_or(600)
}

async fn fetch(
    app: &App,
    base: &str,
    key: &str,
    cursor: i64,
    since: i64,
) -> Result<(Vec<Item>, i64, bool), u64> {
    let list = get(
        app,
        &format!("{base}/IStoreService/GetAppList/v1/"),
        &[
            ("key", key.to_string()),
            ("include_games", "true".into()),
            ("if_modified_since", since.to_string()),
            ("last_appid", cursor.to_string()),
            ("max_results", BATCH.to_string()),
        ],
    )
    .await?;
    let page = serde_json::from_slice::<AppList>(&list)
        .map_err(|_| 3600u64)?
        .response;
    if page.apps.len() > BATCH as usize || (page.have_more_results && page.last_appid <= cursor) {
        return Err(3600);
    }
    if page.apps.is_empty() {
        return Ok((Vec::new(), cursor, false));
    }
    let request = json!({
        "ids": page.apps.iter().map(|a| json!({"appid": a.appid})).collect::<Vec<_>>(),
        "context": {"language": "english", "country_code": "US"},
        "data_request": {"include_tag_count": 20},
    });
    let items = get(
        app,
        &format!("{base}/IStoreBrowseService/GetItems/v1/"),
        &[("input_json", request.to_string())],
    )
    .await?;
    let items = serde_json::from_slice::<Items>(&items)
        .map_err(|_| 3600u64)?
        .response
        .store_items;
    if items.len() > BATCH as usize {
        return Err(3600);
    }
    Ok((items, page.last_appid, page.have_more_results))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tags(ids: &[(u64, i64)]) -> Vec<Tag> {
        ids.iter()
            .map(|&(tagid, weight)| Tag { tagid, weight })
            .collect()
    }
    #[test]
    fn the_heaviest_mapped_tag_picks_the_genre() {
        // Counter-Strike 2: FPS outweighs Strategy.
        assert_eq!(
            classify(&tags(&[(19, 603), (1663, 1160), (9, 382)])),
            Some("fps_battle_royale")
        );
        // Apex Legends: Free to Play and Multiplayer map to nothing; Battle Royale does.
        assert_eq!(
            classify(&tags(&[(113, 900), (3859, 800), (176981, 700), (122, 10)])),
            Some("fps_battle_royale")
        );
        assert_eq!(classify(&tags(&[(19, 900), (21, 800), (492, 700)])), None);
        // A mapped tag below the top eight doesn't count.
        let mut deep: Vec<(u64, i64)> = (0..8).map(|i| (19, 100 - i)).collect();
        deep.push((1663, 1));
        assert_eq!(classify(&tags(&deep)), None);
    }
}
