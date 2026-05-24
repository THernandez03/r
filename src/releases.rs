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

/// Parse the `version = "X.Y.Z-channel (sha date)"` entry from the `[pkg.rust]`
/// section of a Rust channel manifest (TOML text) and return `(version_part, sha)`.
///
/// For nightly: `version_part = "1.90.0-nightly"`, `sha = "abc1234de"`
/// For beta:    `version_part = "1.88.0-beta.3"`,  `sha = "def5678ab"`
fn parse_version_sha_from_manifest(body: &str) -> Option<(String, String)> {
    let mut in_pkg_rust = false;
    for line in body.lines() {
        let line = line.trim();
        // Enter [pkg.rust] section
        if line == "[pkg.rust]" {
            in_pkg_rust = true;
            continue;
        }
        // Leave [pkg.rust] when the next section header starts
        if in_pkg_rust && line.starts_with('[') {
            break;
        }
        if !in_pkg_rust {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version = \"") {
            if let Some(ver_str) = rest.strip_suffix('"') {
                // ver_str = "1.90.0-nightly (abc1234de 2025-02-01)"
                if let Some(paren_pos) = ver_str.find(" (") {
                    let version_part = &ver_str[..paren_pos];
                    let paren_content = &ver_str[paren_pos + 2..];
                    if let Some(sha) = paren_content.split_whitespace().next() {
                        let sha = sha.trim_end_matches(')');
                        if !sha.is_empty() && sha.chars().all(|c| c.is_ascii_hexdigit()) {
                            return Some((version_part.to_string(), sha.to_string()));
                        }
                    }
                }
            }
        }
    }
    None
}

/// Fetch the current nightly version and commit hash from the channel manifest
/// and return a cache key like `"1.90.0-nightly+abc1234de"`. The download URL
/// is selected by detecting `"-nightly"` in the tag (see `arch.rs`).
fn resolve_nightly_tag() -> Result<String> {
    let client = Client::new();
    let body = client
        .get(NIGHTLY_MANIFEST_URL)
        .header("User-Agent", "r-rust-version-manager")
        .send()
        .context("Failed to fetch Rust nightly manifest")?
        .text()
        .context("Failed to read nightly manifest body")?;

    let (version_part, sha) = parse_version_sha_from_manifest(&body)
        .ok_or_else(|| anyhow::anyhow!("Could not parse version from Rust nightly manifest"))?;

    anyhow::ensure!(
        !version_part.is_empty() && !sha.is_empty(),
        "Empty version or SHA in Rust nightly manifest"
    );
    let short_sha = &sha[..sha.len().min(9)];
    Ok(format!("{version_part}+{short_sha}"))
}

/// Fetch the current beta version and commit hash from the channel manifest
/// and return a cache key like `"1.88.0-beta.3+def5678ab"`. The download URL
/// is selected by detecting `"-beta"` in the tag (see `arch.rs`).
fn resolve_beta_tag() -> Result<String> {
    let client = Client::new();
    let body = client
        .get(BETA_MANIFEST_URL)
        .header("User-Agent", "r-rust-version-manager")
        .send()
        .context("Failed to fetch Rust beta manifest")?
        .text()
        .context("Failed to read beta manifest body")?;

    let (version_part, sha) = parse_version_sha_from_manifest(&body)
        .ok_or_else(|| anyhow::anyhow!("Could not parse version from Rust beta manifest"))?;

    anyhow::ensure!(
        !version_part.is_empty() && !sha.is_empty(),
        "Empty version or SHA in Rust beta manifest"
    );
    let short_sha = &sha[..sha.len().min(9)];
    Ok(format!("{version_part}+{short_sha}"))
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
/// - `"nightly"` / `"canary"` / `"next"` / `"edge"` / `"latest"` → `"X.Y.Z-nightly+sha"` (fetches manifest)
/// - `"beta"` → `"X.Y.Z-beta.N+sha"` (fetches manifest)
/// - `"stable"` / `"lts"` / `"current"` / `""` → latest stable from GitHub
/// - `"1.87"` → latest in that minor line (GitHub)
/// - `"1.87.0"` → exact tag — no network needed
pub fn resolve_tag(version_str: &str) -> Result<String> {
    let v = version_str.trim();

    // Reject v-prefixed version strings
    if v.starts_with('v') && v[1..].starts_with(|c: char| c.is_ascii_digit()) {
        anyhow::bail!("No Rust release found matching '{v}'");
    }

    if matches!(v, "nightly" | "canary" | "next" | "edge" | "latest") {
        return resolve_nightly_tag();
    }

    if v == "beta" {
        return resolve_beta_tag();
    }

    // Three-part exact version — skip network
    if v.split('.').count() == 3 && v.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Ok(v.to_string());
    }

    let releases = fetch_releases(100)?;
    resolve_from(v, &releases)
}

/// Pure resolver operating on a pre-fetched slice (used by tests).
pub fn resolve_from(version_str: &str, releases: &[GhRelease]) -> Result<String> {
    let v = version_str.trim();

    // Reject v-prefixed version strings
    if v.starts_with('v') && v[1..].starts_with(|c: char| c.is_ascii_digit()) {
        anyhow::bail!("No Rust release found matching '{v}'");
    }

    if matches!(v, "nightly" | "canary" | "next" | "edge" | "latest") {
        return Ok("nightly".to_string());
    }

    if v == "beta" {
        return Ok("beta".to_string());
    }

    // Latest stable / lts / current aliases
    if v.is_empty() || matches!(v, "stable" | "lts" | "current") {
        return releases
            .iter()
            .find(|r| !r.prerelease)
            .map(|r| r.tag_name.clone())
            .ok_or_else(|| anyhow::anyhow!("No stable Rust release found on GitHub"));
    }

    // Strip trailing `.x` / `.X`
    let prefix = v.trim_end_matches(".x").trim_end_matches(".X");

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
    fn resolve_prefix_with_v_rejected() {
        assert!(resolve_from("v1.86", &stable_releases()).is_err());
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
    fn resolve_exact_version_with_v_rejected() {
        assert!(resolve_from("v1.87.0", &stable_releases()).is_err());
    }

    #[test]
    fn resolve_current_returns_stable() {
        assert_eq!(
            resolve_from("current", &stable_releases()).unwrap(),
            "1.87.0"
        );
    }

    #[test]
    fn parse_version_sha_nightly() {
        let manifest = r#"[pkg.rust]
version = "1.90.0-nightly (abc1234de 2025-02-01)"
"#;
        let result = parse_version_sha_from_manifest(manifest).unwrap();
        assert_eq!(result.0, "1.90.0-nightly");
        assert_eq!(result.1, "abc1234de");
    }

    #[test]
    fn parse_version_sha_beta() {
        // Mirrors real manifest layout: [pkg.cargo] appears first, [pkg.rust] follows.
        let manifest = r#"[pkg.cargo]
version = "0.97.0-beta.3 (bfa14ef47 2025-01-28)"

[pkg.rust]
version = "1.88.0-beta.3 (def5678ab 2025-01-30)"
"#;
        let result = parse_version_sha_from_manifest(manifest).unwrap();
        assert_eq!(result.0, "1.88.0-beta.3");
        assert_eq!(result.1, "def5678ab");
    }

    #[test]
    fn parse_version_sha_missing() {
        assert!(parse_version_sha_from_manifest("date = \"2025-01-30\"").is_none());
    }

    #[test]
    fn parse_version_sha_ignores_pkg_cargo_section() {
        // A manifest where [pkg.rust] is absent must return None even when
        // [pkg.cargo] has a valid version line — the fix for the beta bug.
        let manifest = "[pkg.cargo]\nversion = \"0.97.0-beta.9 (bfa14ef47 2026-05-15)\"\n";
        assert!(parse_version_sha_from_manifest(manifest).is_none());
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
