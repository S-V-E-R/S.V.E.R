//! IPinfo Lite network lookups for viewer integrity (docs/LIVE_STREAMS.md, "Signals"). The file is
//! downloaded daily, checked before it replaces the old one, and read locally; no lookup leaves the
//! server. A missing or stale file means no network signal, never an exclusion.
use crate::App;
use maxminddb::Reader;
use std::{
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime},
};

const SOURCE: &str = "https://ipinfo.io/data/ipinfo_lite.mmdb";
const REFRESH: Duration = Duration::from_secs(24 * 3600);
const STALE: Duration = Duration::from_secs(3 * 24 * 3600);

pub struct Db {
    reader: Reader<Vec<u8>>,
    built: SystemTime,
}

#[derive(Clone, Default)]
pub struct Networks {
    token: String,
    path: Option<PathBuf>,
    db: Arc<RwLock<Option<Db>>>,
}

impl Networks {
    /// `IPINFO_TOKEN` and `IPINFO_DB` (where the file lives); both unset turns network signals off.
    pub fn from_env() -> Result<Self, String> {
        let token = std::env::var("IPINFO_TOKEN").unwrap_or_default();
        let path = std::env::var("IPINFO_DB").unwrap_or_default();
        if token.is_empty() != path.is_empty() {
            return Err("IPINFO_TOKEN and IPINFO_DB must be set together".into());
        }
        let networks = Self {
            token,
            path: (!path.is_empty()).then(|| PathBuf::from(path)),
            db: Arc::default(),
        };
        networks.load();
        Ok(networks)
    }

    /// The viewer's network (`AS15169`), or None when the file is missing, stale or has no entry.
    pub fn asn(&self, ip: IpAddr) -> Option<String> {
        let guard = self.db.read().ok()?;
        let db = guard.as_ref()?;
        if db.built.elapsed().unwrap_or(STALE) >= STALE {
            return None;
        }
        lookup(&db.reader, ip)
    }

    /// Reads the file already on disk, if it opens and answers a known address.
    fn load(&self) {
        let Some(path) = &self.path else { return };
        let (Ok(bytes), Ok(built)) = (
            std::fs::read(path),
            std::fs::metadata(path).and_then(|m| m.modified()),
        ) else {
            return;
        };
        if let Some(reader) = verified(bytes)
            && let Ok(mut db) = self.db.write()
        {
            *db = Some(Db { reader, built });
        }
    }
}

fn lookup(reader: &Reader<Vec<u8>>, ip: IpAddr) -> Option<String> {
    reader
        .lookup(ip.to_canonical())
        .ok()?
        .decode_path::<String>(&maxminddb::path!["asn"])
        .ok()?
}

/// A download is only used if it parses and knows a well-known address's network.
fn verified(bytes: Vec<u8>) -> Option<Reader<Vec<u8>>> {
    let reader = Reader::from_source(bytes).ok()?;
    let probe = lookup(&reader, IpAddr::from([8, 8, 8, 8]))?;
    probe.starts_with("AS").then_some(reader)
}

/// Downloads a new file once a day. Failures keep the current file; the next pass retries.
pub async fn refresh(app: &App) -> Result<(), String> {
    let networks = &app.config.networks;
    let Some(path) = &networks.path else {
        return Ok(());
    };
    let fresh = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().unwrap_or(REFRESH) < REFRESH);
    if fresh {
        if networks.db.read().map(|db| db.is_none()).unwrap_or(true) {
            networks.load();
        }
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("SVER/2.0")
        .build()
        .map_err(|_| "client")?;
    let bytes = client
        .get(SOURCE)
        .bearer_auth(&networks.token)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "download")?
        .bytes()
        .await
        .map_err(|_| "download")?
        .to_vec();
    if verified(bytes.clone()).is_none() {
        return Err("the downloaded file failed its check".into());
    }
    let staging = path.with_extension("mmdb.new");
    std::fs::write(&staging, &bytes).map_err(|_| "write")?;
    std::fs::rename(&staging, path).map_err(|_| "swap")?;
    networks.load();
    eprintln!("ipinfo_event=refresh outcome=ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in: `IPINFO_TEST_DB` points at a real IPinfo Lite file.
    #[test]
    fn real_file_answers_known_networks() {
        let Ok(path) = std::env::var("IPINFO_TEST_DB") else {
            return;
        };
        let reader = verified(std::fs::read(path).unwrap()).expect("file passes its check");
        assert_eq!(
            lookup(&reader, "1.1.1.1".parse().unwrap()).as_deref(),
            Some("AS13335")
        );
        assert_eq!(
            lookup(&reader, "::ffff:8.8.4.4".parse().unwrap()).as_deref(),
            Some("AS15169")
        );
        assert_eq!(
            lookup(&reader, "10.0.0.1".parse().unwrap()),
            None,
            "private address"
        );
        assert!(verified(b"not a database".to_vec()).is_none());
    }
}
