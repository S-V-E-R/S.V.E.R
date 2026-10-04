//! Public progress comes from the roadmap shipped with this API release, never local work.
use axum::Json;
use serde::Serialize;
use sha2::{Digest, Sha256};

const SOURCE: &str = include_str!("../../../../../docs/ROADMAP.md");
const IDS: [&str; 10] = [
    "foundation",
    "login",
    "profiles",
    "live-streams",
    "factions",
    "magnet",
    "support",
    "crowdsync",
    "vods-clips",
    "beacons",
];

#[derive(Serialize)]
pub struct Item {
    id: &'static str,
    number: usize,
    name: String,
    detail: String,
    status: String,
}

#[derive(Serialize)]
pub struct Roadmap {
    revision: String,
    items: Vec<Item>,
}

fn parse(source: &str) -> Result<Roadmap, &'static str> {
    let mut items = Vec::new();
    for line in source.lines() {
        let cells: Vec<_> = line.trim().split('|').map(str::trim).collect();
        if cells.len() < 2 || !cells[0].is_empty() || cells[1].parse::<usize>().is_err() {
            continue;
        }
        let number = cells[1].parse::<usize>().map_err(|_| "Invalid module")?;
        if cells.len() != 6 || number != items.len() || number >= IDS.len() {
            return Err("Invalid roadmap table");
        }
        if cells[2].is_empty()
            || cells[3].is_empty()
            || !matches!(cells[4], "Done" | "In progress" | "Started" | "Planned")
        {
            return Err("Invalid roadmap item");
        }
        items.push(Item {
            id: IDS[number],
            number,
            name: cells[2].into(),
            detail: cells[3].into(),
            status: cells[4].into(),
        });
    }
    if items.len() != IDS.len() {
        return Err("Incomplete roadmap");
    }
    let data = serde_json::to_vec(&items).map_err(|_| "Invalid roadmap")?;
    Ok(Roadmap {
        revision: Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        items,
    })
}

pub async fn read() -> crate::Result<Json<Roadmap>> {
    parse(SOURCE)
        .map(Json)
        .map_err(|_| crate::Error::internal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_table_is_complete_and_changes_are_observable() {
        let initial = parse(SOURCE).unwrap();
        assert_eq!(initial.items.len(), 10);
        let changed = parse(&SOURCE.replacen("| Done |", "| In progress |", 1)).unwrap();
        assert_ne!(initial.revision, changed.revision);
        assert_eq!(changed.items[0].status, "In progress");
        assert_eq!(
            parse(&format!("{SOURCE}\nPrivate-looking text outside the table"))
                .unwrap()
                .revision,
            initial.revision
        );
    }

    #[test]
    fn malformed_or_incomplete_progress_is_never_published() {
        for source in [
            String::new(),
            SOURCE.replace("| Planned |", "| Almost done |"),
            SOURCE.replace("| 9 |", "| 8 |"),
            SOURCE.replace("| 9 |", "| 10 |"),
            SOURCE.replace("| 0 | Foundation |", "| 0 | |"),
        ] {
            assert!(parse(&source).is_err());
        }
    }
}
