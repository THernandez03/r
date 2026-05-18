/// Returns the Rust target triple for the current platform.
/// Rust release filenames use the pattern: `rust-{version}-{target}.tar.gz`
/// e.g. `rust-1.87.0-x86_64-unknown-linux-gnu.tar.gz`
#[must_use]
pub const fn target() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";
    #[cfg(not(any(
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "macos", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "windows", any(target_arch = "x86_64", target_arch = "aarch64")),
    )))]
    return "x86_64-unknown-linux-gnu"; // fallback
}

/// Build the download URL for a specific Rust toolchain tag.
///
/// Tags encode the channel:
/// - `"nightly-YYYY-MM-DD"` → `https://static.rust-lang.org/dist/rust-nightly-{target}.tar.gz`
/// - `"beta"` → `https://static.rust-lang.org/dist/rust-beta-{target}.tar.gz`
/// - `"1.87.0"` → `https://static.rust-lang.org/dist/rust-1.87.0-{target}.tar.gz`
#[must_use]
pub fn download_url(tag: &str) -> String {
    let tgt = target();
    if tag.starts_with("nightly") {
        format!("https://static.rust-lang.org/dist/rust-nightly-{tgt}.tar.gz")
    } else if tag.starts_with("beta") {
        format!("https://static.rust-lang.org/dist/rust-beta-{tgt}.tar.gz")
    } else {
        format!("https://static.rust-lang.org/dist/rust-{tag}-{tgt}.tar.gz")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_non_empty() {
        assert!(!target().is_empty());
    }

    #[test]
    fn target_contains_dash() {
        assert!(target().contains('-'));
    }

    #[test]
    fn target_is_known_triple() {
        let known = [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        assert!(known.contains(&target()), "unexpected target: {}", target());
    }

    #[test]
    fn download_url_stable_contains_version() {
        let url = download_url("1.87.0");
        assert!(url.contains("1.87.0"), "url: {url}");
    }

    #[test]
    fn download_url_stable_starts_with_rust_lang() {
        let url = download_url("1.87.0");
        assert!(
            url.starts_with("https://static.rust-lang.org/dist/rust-"),
            "url: {url}"
        );
    }

    #[test]
    fn download_url_stable_ends_with_tar_gz() {
        let url = download_url("1.87.0");
        assert!(url.ends_with(".tar.gz"), "url: {url}");
    }

    #[test]
    fn download_url_nightly_uses_nightly_slug() {
        let url = download_url("nightly-2024-11-15");
        assert!(url.contains("rust-nightly-"), "url: {url}");
        assert!(
            !url.contains("nightly-2024"),
            "url should not embed date: {url}"
        );
    }

    #[test]
    fn download_url_beta_uses_beta_slug() {
        let url = download_url("beta");
        assert!(url.contains("rust-beta-"), "url: {url}");
    }

    #[test]
    fn download_url_contains_target() {
        let url = download_url("1.87.0");
        assert!(url.contains(target()), "url: {url}");
    }
}
