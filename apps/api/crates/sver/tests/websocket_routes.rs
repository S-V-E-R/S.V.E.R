//! Every API WebSocket route must be in nginx's upgraded location, or production answers the
//! upgrade with 400 while local tests (no nginx) pass. Missed twice: overlays, then DMs and events.
use std::{fs, path::Path};

fn sources(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap().map(|e| e.unwrap()) {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(fs::read_to_string(path).unwrap());
        }
    }
}

#[test]
fn websocket_routes_are_upgraded_by_nginx() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let nginx = fs::read_to_string(root.join("../../../../infra/nginx-sver-stage.conf")).unwrap();
    // The one location that sets Upgrade: `location ~ ^/api/(a|b|...)$ {`
    let location = nginx
        .lines()
        .find(|l| l.trim_start().starts_with("location ~ ^/api/(") && l.contains("/ws"))
        .expect("nginx WebSocket location");
    let inner = &location[location.find("^/api/(").unwrap() + 7..location.find(")$").unwrap()];
    let upgraded: Vec<&str> = inner.split('|').collect();

    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    let mut routes = Vec::new();
    for file in &files {
        for chunk in file.split("async fn ").skip(1) {
            let name = &chunk[..chunk.find('(').unwrap_or(0)];
            let params = &chunk[..chunk.find("->").unwrap_or(chunk.len())];
            if name.is_empty() || !params.contains("WebSocketUpgrade") {
                continue;
            }
            let call = format!("get({name}))");
            for line in file
                .lines()
                .filter(|l| l.contains(".route(\"") && l.contains(&call))
            {
                let path = line.split('"').nth(1).unwrap();
                routes.push(path.to_owned());
            }
        }
    }
    assert!(routes.len() >= 7, "found only {routes:?}");
    for path in routes {
        // `{lane}` in an axum path is `[^/]+` in the nginx pattern.
        let pattern: String = path
            .trim_start_matches("/api/")
            .split('/')
            .map(|s| if s.starts_with('{') { "[^/]+" } else { s })
            .collect::<Vec<_>>()
            .join("/");
        assert!(
            upgraded.contains(&pattern.as_str()),
            "{path} upgrades to a WebSocket but infra/nginx-sver-stage.conf doesn't upgrade it"
        );
    }
}
