//! Route-walk check for root-level channel pages (docs/PROFILES.md, "Reserved names").
//! Every top-level route that could be a valid username must be on the reserved list,
//! so no user can claim a name that a site route shadows.
use std::{
    fs,
    path::{Path, PathBuf},
};
use sver::{reserved, security as sec};

/// Next.js metadata files that create a root route from their file stem.
const METADATA_FILES: &[&str] = &[
    "favicon",
    "icon",
    "apple-icon",
    "opengraph-image",
    "twitter-image",
    "robots",
    "sitemap",
    "manifest",
];

#[derive(Default, Debug)]
struct Walk {
    /// (first path segment, where it came from)
    segments: Vec<(String, String)>,
    root_dynamic: Vec<String>,
}

fn stem(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

fn walk_app(dir: &Path, walk: &mut Walk) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("Cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let source = format!("web/app: {}", entry.path().display());
        if entry.file_type().unwrap().is_dir() {
            if name.starts_with('(') && name.ends_with(')') && !name.starts_with("(.") {
                // Route group: its children are still top-level routes.
                walk_app(&entry.path(), walk);
            } else if name.starts_with('@') {
                // Parallel route slot: adds no path segment.
                walk_app(&entry.path(), walk);
            } else if name.starts_with('_') {
                // Private folder: not routable.
            } else if name.starts_with('[') {
                walk.root_dynamic.push(name);
            } else if name.starts_with('(') {
                panic!("Intercepting route {name} needs an explicit reserved-route rule");
            } else {
                walk.segments.push((name, source));
            }
        } else if METADATA_FILES.contains(&stem(&name)) {
            walk.segments.push((stem(&name).to_owned(), source));
        }
    }
}

fn walk_public(dir: &Path, walk: &mut Walk) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.map(|e| e.unwrap()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let segment = if entry.file_type().unwrap().is_dir() {
            name.clone()
        } else {
            stem(&name).to_owned()
        };
        walk.segments.push((segment, format!("web/public: {name}")));
    }
}

/// First path segment of every `source:` string in the Next.js config (redirects, rewrites and
/// headers). Parameter or pattern segments such as `/:path*` cannot shadow a username.
fn walk_next_config(file: &Path, walk: &mut Walk) {
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    let mut rest = text.as_str();
    while let Some(index) = rest.find("source") {
        rest = &rest[index + "source".len()..];
        let after = rest.trim_start();
        let Some(after) = after.strip_prefix(':') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\'' | '`'))
        else {
            continue;
        };
        let body = &after[1..];
        let Some(end) = body.find(quote) else {
            continue;
        };
        let value = &body[..end];
        let first = value
            .trim_start_matches('/')
            .split('/')
            .next()
            .unwrap_or("");
        if first.is_empty() || first.starts_with(':') || first.starts_with('(') {
            continue;
        }
        walk.segments
            .push((first.to_owned(), format!("next.config.ts: source {value}")));
    }
}

fn walk_web(web: &Path) -> Walk {
    let mut walk = Walk::default();
    walk_app(&web.join("app"), &mut walk);
    walk_public(&web.join("public"), &mut walk);
    walk_next_config(&web.join("next.config.ts"), &mut walk);
    walk
}

/// Problems that must fail the build: unreserved claimable route names, or a second root
/// dynamic segment competing with `/[username]`.
fn problems(web: &Path) -> Vec<String> {
    let walk = walk_web(web);
    let mut problems: Vec<String> = walk
        .segments
        .iter()
        .filter(|(segment, _)| {
            reserved::is_username_shaped(segment) && !reserved::is_listed(segment)
        })
        .map(|(segment, source)| format!("{segment} is not reserved ({source})"))
        .collect();
    if walk.root_dynamic.len() > 1 {
        problems.push(format!(
            "More than one root dynamic segment: {}",
            walk.root_dynamic.join(", ")
        ));
    }
    problems
}

fn repository_web() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../web")
}

#[test]
fn every_top_level_route_is_reserved() {
    let walk = walk_web(&repository_web());
    assert!(
        walk.segments.iter().any(|(s, _)| s == "login")
            && walk.segments.iter().any(|(s, _)| s == "api"),
        "Route walk must see the auth routes and the API rewrite: {walk:?}"
    );
    assert_eq!(
        walk.root_dynamic,
        vec!["[username]".to_owned()],
        "The only root dynamic segment is the channel page"
    );
    let problems = problems(&repository_web());
    assert!(
        problems.is_empty(),
        "Reserve these route names: {problems:#?}"
    );
    for (segment, source) in &walk.segments {
        if reserved::is_username_shaped(segment) {
            assert!(
                sec::validate_username(segment).is_err(),
                "Signup must refuse route name {segment} ({source})"
            );
        }
    }
}

#[test]
fn route_walk_flags_unreserved_entries() {
    let root =
        std::env::temp_dir().join(format!("sver-route-walk-{}", uuid::Uuid::new_v4().simple()));
    let web = root.join("web");
    for dir in [
        "app/login",
        "app/(group)/zzz_grouped",
        "app/zzz_plain",
        "app/_private",
        "app/[username]",
        "app/[other]",
        "app/oauth-signup",
        "public/zzz_assets",
    ] {
        fs::create_dir_all(web.join(dir)).unwrap();
    }
    fs::write(web.join("app/page.tsx"), "").unwrap();
    fs::write(web.join("app/zzz_helper.ts"), "").unwrap();
    fs::write(web.join("app/robots.ts"), "").unwrap();
    fs::write(web.join("app/icon.png"), "").unwrap();
    fs::write(web.join("public/zzz_file.txt"), "").unwrap();
    fs::write(
        web.join("next.config.ts"),
        "redirects() { return [{ source: \"/zzz_redirect/:p*\", destination: \"/\" }, { source: '/:path*', destination: '/' }, { source: `/api/:path*` }]; }",
    )
    .unwrap();
    let found = problems(&web);
    fs::remove_dir_all(&root).unwrap();
    for expected in [
        "zzz_grouped is not reserved",
        "zzz_plain is not reserved",
        "icon is not reserved",
        "zzz_assets is not reserved",
        "zzz_file is not reserved",
        "zzz_redirect is not reserved",
        "More than one root dynamic segment",
    ] {
        assert!(
            found.iter().any(|p| p.starts_with(expected)),
            "Expected a problem starting with {expected:?}, got {found:#?}"
        );
    }
    for allowed in [
        "login ",
        "robots ",
        "api ",
        "oauth-signup",
        "_private",
        "zzz_helper",
        "page ",
    ] {
        assert!(
            !found.iter().any(|p| p.starts_with(allowed)),
            "{allowed:?} must not be flagged: {found:#?}"
        );
    }
    assert_eq!(found.len(), 7, "{found:#?}");
}

#[test]
fn reserved_list_is_normalized_and_enforced() {
    let mut seen = std::collections::HashSet::new();
    for name in reserved::RESERVED {
        assert!(
            name.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
            "{name} must be lowercase ASCII"
        );
        assert!(seen.insert(*name), "{name} is listed twice");
    }
    for name in [
        "Login",
        "SETTINGS",
        "following",
        "Studio",
        "admin",
        "Support",
        "SVER",
        "s_v_e_r",
        "5ver",
        "myria",
        "Gl1nt",
        "browse",
        "live",
        "watch",
        "dashboard",
        "wallet",
        "_next",
        "forgot",
        "reset",
        "verify",
        "mfa",
        "account",
        "x_admin_x",
        "n1gg3r",
        "a_1488_b",
    ] {
        assert!(reserved::is_reserved(name), "{name} must be reserved");
        assert!(
            sec::validate_username(name).is_err(),
            "{name} must be refused"
        );
    }
    for name in [
        "JoeTheChode",
        "First_User",
        "AnotherName",
        "streamer_42",
        "n_e_x_t",
    ] {
        assert!(!reserved::is_reserved(name), "{name} must stay available");
        assert!(
            sec::validate_username(name).is_ok(),
            "{name} must be accepted"
        );
    }
}
