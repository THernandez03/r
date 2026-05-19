use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/rust-lang/rust/releases";
const NIGHTLY_MANIFEST_URL: &str = "https://static.rust-lang.org/dist/channel-rust-nightly.toml";
const BETA_MANIFEST_URL: &str = "https://static.rust-lang.org/dist/channel-rust-beta.toml";

#[derive(Debug, Deserialize, Clone)]
pub struct GhRelease {
    pub tag_name: String,
    pub prerelease: bool,
}

/// Fetch the current nightly date from the channel manifest and return a
/// stable cache key like `"nightly-2024-11-15"`. The download URL always uses
/// `rust-nightly-{target}.tar.gz` regardless of the date embedded in the key.
fn resolve_nightly_tag() -> Result<String> {
    let client = Client::new();
    let body = client
        .get(NIGHTLY_MANIFEST_URL)
        .header("User-Agent", "r-rust-version-manager")
        .send()
        .context("Failed to fetch Rust nightly manifest")?
        .text()
        .context("Failed to read nightly manifest body")?;

    let date = body
        .lines()
        .find_map(|line| line.strip_prefix("date = \"")?.strip_suffix('"'))
        .ok_or_else(|| anyhow::anyhow!("Could not parse date from nightly manifest"))?;

    anyhow::ensure!(!date.is_empty(), "Nightly manifest date was empty");
    Ok(format!("nightly-{date}"))
}

/// Return `"beta"` as the canonical cache key for the current beta toolchain.
fn resolve_beta_tag() -> Result<String> {
    // Validate the manifest is reachable so we fail fast on network errors,
    // but the tag itself is always "beta" (the URL uses rust-beta-{target}).
    let client = Client::new();
    client
        .get(BETA_MANIFEST_URL)
        .header("User-Agent", "r-rust-version-manager")
        .send()
        .context("Failed to reach Rust beta manifest")?;
    Ok("beta".to_string())
}

/// Fetch recent releases from the GitHub API.
pub fn fetch_releases(per_page: u32) -> Result<Vec<GhRelease>> {
    let client = Client::new();
    let url = format!("{GITHUB_RELEASES_URL}?per_page={per_page}");
    let releases: Vec<GhRelease> = client
        .get(&url)
        .header("User-Agent", "r-rust-version-manager")
        .send()
        .context("Failed to fetch Rust releases from GitHub")?
        .json()
        .context("Failed to parse GitHub releases JSON")?;
    Ok(releases)
}

/// Print recent stable Rust releases (latest 20).
pub fn list_remote() -> Result<()> {
    let releases = fetch_releases(20)?;
    println!("Available Rust versions (recent 20):");
    for r in &releases {
        let pre = if r.prerelease { " (pre-release)" } else { "" };
        println!("  {}{}", r.tag_name, pre);
    }
    Ok(())
}

/// Resolve a user-supplied version string to a cache tag.
///
/// Aliases:
/// - `"nightly"` / `"canary"` / `"next"` / `"edge"` / `"latest"` → `"nightly-YYYY-MM-DD"` (fetches manifest)
/// - `"beta"` → `"beta"`
/// - `"stable"` / `"lts"` / `""` → latest stable from GitHub
/// - `"1.87"` / `"v1.87"` → latest in that minor line (GitHub)
/// - `"1.87.0"` / `"v1.87.0"` → exact tag — no network needed
pub fn resolve_tag(version_str: &str) -> Result<String> {
    let v = version_str.trim();

    if matches!(v, "nightly" | "canary" | "next" | "edge" | "latest") {
        return resolve_nightly_tag();
    }

    if v == "beta" {
        return resolve_beta_tag();
    }

    let bare = v.strip_prefix('v').unwrap_or(v);

    // Three-part exact version — skip network
    if bare.split('.').count() == 3 && bare.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Ok(bare.to_string());
    }

    let releases = fetch_releases(100)?;
    resolve_from(v, &releases)
}

/// Pure resolver operating on a pre-fetched slice (used by tests).
pub fn resolve_from(version_str: &str, releases: &[GhRelease]) -> Result<String> {
    let v = version_str.trim();

    if matches!(v, "nightly" | "canary" | "next" | "edge" | "latest") {
        return Ok("nightly".to_string());
    }

    if v == "beta" {
        return Ok("beta".to_string());
    }

    let bare = v.strip_prefix('v').unwrap_or(v);

    // Latest stable / lts aliases
    if bare.is_empty() || matches!(bare, "stable" | "lts") {
        return releases
            .iter()
            .find(|r| !r.prerelease)
            .map(|r| r.tag_name.clone())
            .ok_or_else(|| anyhow::anyhow!("No stable Rust release found on GitHub"));
    }

    // Strip trailing `.x` / `.X`
    let prefix = bare.trim_end_matches(".x").trim_end_matches(".X");

    // Prefix match: e.g. "1.87" matches "1.87.0"
    let needle = format!("{prefix}.");
    releases
        .iter()
        .find(|r| !r.prerelease && (r.tag_name.starts_with(&needle) || r.tag_name == prefix))
        .map(|r| r.tag_name.clone())
        .ok_or_else(|| anyhow::anyhow!("No stable Rust release found matching '{version_str}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_release(tag: &str, prerelease: bool) -> GhRelease {
        GhRelease {
            tag_name: tag.to_string(),
            prerelease,
        }
    }

    fn stable_releases() -> Vec<GhRelease> {
        vec![
            make_release("1.87.0", false),
            make_release("1.86.1", false),
            make_release("1.86.0", false),
            make_release("1.85.0", false),
            make_release("1.85.0-beta.1", true),
        ]
    }

    #[test]
    fn resolve_stable_returns_first_stable() {
        assert_eq!(
            resolve_from("stable", &stable_releases()).unwrap(),
            "1.87.0"
        );
    }

    #[test]
    fn resolve_latest_returns_nightly() {
        assert_eq!(
            resolve_from("latest", &stable_releases()).unwrap(),
            "nightly"
        );
    }

    #[test]
    fn resolve_lts_returns_first_stable() {
        assert_eq!(resolve_from("lts", &stable_releases()).unwrap(), "1.87.0");
    }

    #[test]
    fn resolve_nightly_alias() {
        assert_eq!(
            resolve_from("nightly", &stable_releases()).unwrap(),
            "nightly"
        );
    }

    #[test]
    fn resolve_canary_alias() {
        assert_eq!(
            resolve_from("canary", &stable_releases()).unwrap(),
            "nightly"
        );
    }

    #[test]
    fn resolve_next_alias() {
        assert_eq!(resolve_from("next", &stable_releases()).unwrap(), "nightly");
    }

    #[test]
    fn resolve_edge_alias() {
        assert_eq!(resolve_from("edge", &stable_releases()).unwrap(), "nightly");
    }

    #[test]
    fn resolve_beta_alias() {
        assert_eq!(resolve_from("beta", &stable_releases()).unwrap(), "beta");
    }

    #[test]
    fn resolve_prefix_minor() {
        assert_eq!(resolve_from("1.86", &stable_releases()).unwrap(), "1.86.1");
    }

    #[test]
    fn resolve_prefix_with_v() {
        assert_eq!(resolve_from("v1.86", &stable_releases()).unwrap(), "1.86.1");
    }

    #[test]
    fn resolve_prefix_with_x_notation() {
        assert_eq!(
            resolve_from("1.86.x", &stable_releases()).unwrap(),
            "1.86.1"
        );
    }

    #[test]
    fn resolve_exact_version() {
        assert_eq!(
            resolve_from("1.87.0", &stable_releases()).unwrap(),
            "1.87.0"
        );
    }

    #[test]
    fn resolve_exact_version_with_v() {
        assert_eq!(
            resolve_from("v1.87.0", &stable_releases()).unwrap(),
            "1.87.0"
        );
    }

    #[test]
    fn resolve_skips_prerelease() {
        assert_eq!(resolve_from("1.85", &stable_releases()).unwrap(), "1.85.0");
    }

    #[test]
    fn resolve_errors_on_unknown() {
        assert!(resolve_from("99", &stable_releases()).is_err());
    }

    #[test]
    fn resolve_errors_on_empty_list() {
        assert!(resolve_from("stable", &[]).is_err());
    }
}
