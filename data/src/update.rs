//! App updates: the version this build is, and whether the site offers a newer one.
//!
//! `bun run release <version>` (scripts/release.sh) publishes `latest.json` next to the Mac
//! download. The Mac app fetches it once at launch (the only request the app makes by itself;
//! it sends nothing about the user or the keyboard) and the Keyboard screen offers the
//! download when it names a newer version. The web app is always the latest and never asks.

use serde_json::{json, Value as Json};

/// This build's version: the workspace version in `Cargo.toml`, which a release sets.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The site, the only origin the app may fetch from.
pub const SITE: &str = "https://remap75.app";
/// What the latest release is: `{ "version", "url", "notes", "released" }`.
pub const FEED: &str = "https://remap75.app/latest.json";
/// Where the Mac app is downloaded when the feed names nowhere on the site.
pub const DOWNLOAD: &str = "https://remap75.app/download/Remap75.dmg";

/// A version's numbers: `1.2.10` → `[1, 2, 10]` (anything after `-` or `+` is ignored).
fn numbers(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let v: Option<Vec<u64>> = core.split('.').map(|n| n.parse().ok()).collect();
    v.filter(|v| !v.is_empty())
}

/// Whether `latest` is a later version than `current` (`0.10.0` is later than `0.9.1`).
pub fn newer(latest: &str, current: &str) -> bool {
    match (numbers(latest), numbers(current)) {
        (Some(mut l), Some(mut c)) => {
            let n = l.len().max(c.len());
            l.resize(n, 0);
            c.resize(n, 0);
            l > c
        }
        _ => false,
    }
}

/// `AppUpdate { version, available, latest, notes, url }` with nothing newer known.
pub fn none() -> Json {
    json!({ "version": VERSION, "available": false, "latest": "", "notes": "", "url": "" })
}

/// `AppUpdate` from the feed's bytes, for a build at version `current`. A feed that can't be
/// read, or names nothing newer, is no update; a download anywhere but the site is not followed.
pub fn from_feed(body: &[u8], current: &str) -> Json {
    let Ok(feed) = serde_json::from_slice::<Json>(body) else {
        return none();
    };
    let latest = feed["version"].as_str().unwrap_or_default();
    if !newer(latest, current) {
        return none();
    }
    let url = feed["url"].as_str().filter(|u| u.starts_with(&format!("{SITE}/"))).unwrap_or(DOWNLOAD);
    let notes: String = feed["notes"].as_str().unwrap_or_default().chars().take(400).collect();
    json!({ "version": current, "available": true, "latest": latest, "notes": notes, "url": url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.1"));
        assert!(newer("v1.0", "0.9.9"));
        assert!(newer("1.0.1", "1.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("1.0.0-beta", "1.0.0"));
        assert!(!newer("", "0.1.0"));
        assert!(!newer("latest", "0.1.0"));
    }

    #[test]
    fn the_feed_offers_only_a_newer_version_from_the_site() {
        let feed = br#"{"version":"0.2.0","url":"https://remap75.app/download/Remap75-0.2.0.dmg","notes":"Flash Mac keys."}"#;
        let u = from_feed(feed, "0.1.0");
        assert_eq!((&u["available"], &u["latest"], &u["notes"]), (&json!(true), &json!("0.2.0"), &json!("Flash Mac keys.")));
        assert_eq!(u["url"], "https://remap75.app/download/Remap75-0.2.0.dmg");
        assert_eq!(from_feed(feed, "0.2.0")["available"], false);
        assert_eq!(from_feed(b"<html>not found</html>", "0.1.0")["available"], false);
        // A download elsewhere is replaced with the site's.
        let elsewhere = br#"{"version":"9.0.0","url":"https://remap75.app.evil.example/x.dmg"}"#;
        assert_eq!(from_feed(elsewhere, "0.1.0")["url"], DOWNLOAD);
        assert_eq!(none()["version"], VERSION);
    }
}
