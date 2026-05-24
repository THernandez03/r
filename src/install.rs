use anyhow::{Context, Result};
use std::fs;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::process::Command;

use console::style;

use crate::{arch, cache, releases, symlink};

/// Returns `true` if the input is a symbolic alias resolved entirely via network.
fn is_alias(s: &str) -> bool {
    matches!(
        s,
        "lts" | "stable" | "current" | "latest" | "canary" | "nightly" | "next" | "edge" | "beta"
    )
}

/// Returns `true` if the input looks like a bare version number.
fn looks_like_version(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'x' | 'X'))
}

/// Returns `true` if the input looks like a git commit SHA (7-40 hex chars
/// with at least one letter `a`-`f`).
fn is_sha_input(s: &str) -> bool {
    let n = s.len();
    (7..=40).contains(&n)
        && s.chars().all(|c| c.is_ascii_hexdigit())
        && s.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
}

/// Extract the base version (before any `+sha`) and the optional SHA from a
/// resolved tag. Channel suffixes like `-nightly`, `-beta.3` are stripped at
/// the first `-`.
fn extract_ver_sha(tag: &str) -> (String, Option<&str>) {
    let (ver_part, sha) = tag
        .split_once('+')
        .map_or((tag, None), |(v, s)| (v, Some(s)));
    let clean_ver = ver_part.split('-').next().unwrap_or(ver_part).to_string();
    (clean_ver, sha)
}

/// Query the installed rustc binary to determine the canonical cache key.
///
/// `rustc 1.87.0 (17067e9ac 2025-01-13)` → `("1.87.0", Some("17067e9ac"))`\
/// `rustc 1.90.0-nightly (abc1234de ...)` → `("1.90.0", Some("abc1234de"))`
fn query_binary_version(binary_path: &Path) -> Result<(String, Option<String>)> {
    let out = Command::new(binary_path)
        .arg("--version")
        .output()
        .context("Failed to run rustc --version")?;
    let raw = String::from_utf8_lossy(&out.stdout);
    let line = raw.trim();
    // "rustc 1.87.0 (17067e9ac 2025-01-13)"
    let rest = line.strip_prefix("rustc ").unwrap_or(line);
    let ver_part = rest.split_whitespace().next().unwrap_or("");
    // Strip channel suffix: "1.90.0-nightly" → "1.90.0", "1.88.0-beta.3" → "1.88.0"
    let clean_ver = ver_part.split('-').next().unwrap_or(ver_part).to_string();
    // Extract SHA from the parenthesised portion: "(17067e9ac 2025-01-13)"
    let sha_opt = rest
        .find('(')
        .and_then(|pos| {
            rest[pos + 1..]
                .split_whitespace()
                .next()
                .map(str::to_string)
        })
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s[..s.len().min(9)].to_string());
    Ok((clean_ver, sha_opt))
}

/// Activate an already-cached version (update the symlink).
fn activate_cached(tag: &str) -> Result<()> {
    if symlink::active_version().as_deref() == Some(tag) {
        println!(
            "{} Rust {} is already the active version.",
            style("\u{2713}").green().bold(),
            style(tag).cyan().bold(),
        );
        return Ok(());
    }
    let from = symlink::active_version();
    match &from {
        Some(f) => println!(
            "{} Activating Rust {} \u{2192} {}...",
            style("\u{25c6}").magenta(),
            style(f).cyan().bold(),
            style(tag).cyan().bold(),
        ),
        None => println!(
            "{} Activating Rust {}...",
            style("\u{25c6}").magenta(),
            style(tag).cyan().bold(),
        ),
    }
    symlink::activate(tag)?;
    println!(
        "{} Installed Rust {} successfully.",
        style("\u{2713}").green().bold(),
        style(tag).cyan().bold(),
    );
    Ok(())
}

/// Install a Rust toolchain and activate it.
pub fn install(version_str: &str) -> Result<()> {
    let v = version_str.trim();

    // 1. Pre-resolve cache check — skip network for version/SHA inputs
    if !is_alias(v) {
        if is_sha_input(v) {
            if let Some(cached) = cache::find_by_sha(v) {
                return activate_cached(&cached);
            }
        } else if looks_like_version(v) {
            let prefix = v.trim_end_matches(".x").trim_end_matches(".X");
            if let Some(cached) = cache::find_by_version_prefix(prefix) {
                return activate_cached(&cached);
            }
        }
    }

    // 2. Resolve via network
    let tag = releases::resolve_tag(v)?;

    // 3. Post-resolve cache check
    {
        let (ver_prefix, tag_sha) = extract_ver_sha(&tag);
        if let Some(cached) = cache::find_by_version_prefix(&ver_prefix) {
            let sha_ok = match (tag_sha, cache::cache_key_sha(&cached)) {
                (Some(ts), Some(cs)) => cache::sha_matches(cs, ts),
                (None, _) => true,
                (Some(_), None) => false,
            };
            if sha_ok {
                return activate_cached(&cached);
            }
        }
    }

    // 4. Download if not already cached
    if !cache::is_cached(&tag) {
        println!(
            "{} Downloading Rust {}...",
            style("\u{2b07}").cyan(),
            style(&tag).cyan().bold(),
        );
        let url = arch::download_url(&tag);
        download_version(&url, &tag)?;
    }

    // 5. Query the installed binary to get the canonical cache key
    let binary = cache::rustc_binary(&tag);
    let canonical = match query_binary_version(&binary) {
        Ok((ver, sha_opt)) => sha_opt.map_or_else(|| ver.clone(), |s| format!("{ver}+{s}")),
        Err(_) => tag.clone(),
    };
    if canonical != tag {
        cache::rename_version(&tag, &canonical)?;
    }

    activate_cached(&canonical)
}

/// Download a version into cache without activating it.
pub fn download_only(version_str: &str) -> Result<()> {
    let v = version_str.trim();

    // Pre-resolve cache check
    if !is_alias(v) {
        if is_sha_input(v) {
            if let Some(cached) = cache::find_by_sha(v) {
                println!("Version {cached} is already cached.");
                return Ok(());
            }
        } else if looks_like_version(v) {
            let prefix = v.trim_end_matches(".x").trim_end_matches(".X");
            if let Some(cached) = cache::find_by_version_prefix(prefix) {
                println!("Version {cached} is already cached.");
                return Ok(());
            }
        }
    }

    let tag = releases::resolve_tag(v)?;

    // Post-resolve cache check
    {
        let (ver_prefix, tag_sha) = extract_ver_sha(&tag);
        if let Some(cached) = cache::find_by_version_prefix(&ver_prefix) {
            let sha_ok = match (tag_sha, cache::cache_key_sha(&cached)) {
                (Some(ts), Some(cs)) => cache::sha_matches(cs, ts),
                (None, _) => true,
                (Some(_), None) => false,
            };
            if sha_ok {
                println!("Version {cached} is already cached.");
                return Ok(());
            }
        }
    }

    if cache::is_cached(&tag) {
        println!("Version {tag} is already cached.");
        return Ok(());
    }
    println!("Downloading Rust {tag}...");
    let url = arch::download_url(&tag);
    download_version(&url, &tag)?;
    let binary = cache::rustc_binary(&tag);
    if let Ok((ver, sha_opt)) = query_binary_version(&binary) {
        let canonical = sha_opt.map_or_else(|| ver.clone(), |s| format!("{ver}+{s}"));
        if canonical != tag {
            cache::rename_version(&tag, &canonical)?;
        }
    }
    Ok(())
}

/// Run a specific cached Rust version's `rustc` with given arguments.
pub fn run(version_str: &str, args: &[String]) -> Result<()> {
    let v = version_str.trim();

    // Pre-resolve cache check
    if !is_alias(v) {
        if is_sha_input(v) {
            if let Some(cached) = cache::find_by_sha(v) {
                return run_cached(&cached, args);
            }
        } else if looks_like_version(v) {
            let prefix = v.trim_end_matches(".x").trim_end_matches(".X");
            if let Some(cached) = cache::find_by_version_prefix(prefix) {
                return run_cached(&cached, args);
            }
        }
    }

    let tag = releases::resolve_tag(v)?;

    // Post-resolve cache check
    let resolved_tag = {
        let (ver_prefix, tag_sha) = extract_ver_sha(&tag);
        if let Some(cached) = cache::find_by_version_prefix(&ver_prefix) {
            let sha_ok = match (tag_sha, cache::cache_key_sha(&cached)) {
                (Some(ts), Some(cs)) => cache::sha_matches(cs, ts),
                (None, _) => true,
                (Some(_), None) => false,
            };
            if sha_ok {
                return run_cached(&cached, args);
            }
        }
        tag
    };

    if !cache::is_cached(&resolved_tag) {
        println!("Version {resolved_tag} is not cached. Downloading...");
        let url = arch::download_url(&resolved_tag);
        download_version(&url, &resolved_tag)?;
    }

    let binary = cache::rustc_binary(&resolved_tag);
    let canonical = match query_binary_version(&binary) {
        Ok((ver, sha_opt)) => sha_opt.map_or_else(|| ver.clone(), |s| format!("{ver}+{s}")),
        Err(_) => resolved_tag.clone(),
    };
    if canonical != resolved_tag {
        cache::rename_version(&resolved_tag, &canonical)?;
    }
    run_cached(&canonical, args)
}

fn run_cached(tag: &str, args: &[String]) -> Result<()> {
    let binary = cache::rustc_binary(tag);
    let status = Command::new(&binary)
        .args(args)
        .status()
        .with_context(|| format!("Failed to run rustc {tag}"))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn download_version(url: &str, tag: &str) -> Result<()> {
    let dest_dir = cache::version_dir(tag);
    fs::create_dir_all(&dest_dir).context("Failed to create cache directory")?;

    let tmp_archive = dest_dir.with_extension("tar.gz");
    let tmp_extract = dest_dir.with_extension("extract");

    // Download the tarball
    {
        let client = reqwest::blocking::Client::new();
        let mut resp = client
            .get(url)
            .header("User-Agent", "r-rust-version-manager")
            .send()
            .context("HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            fs::remove_dir_all(&dest_dir).ok();
            anyhow::bail!("Download failed: server returned HTTP {status} for {url}");
        }

        let total = resp.content_length().unwrap_or(0);
        let file = fs::File::create(&tmp_archive).context("Failed to create temp file")?;
        let mut writer = BufWriter::new(file);

        let mut downloaded = 0u64;
        let mut buf = vec![0u8; 65536];
        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
            downloaded += n as u64;
            if total > 0 {
                if let Some(pct) = downloaded.saturating_mul(100).checked_div(total) {
                    print!("\r  {downloaded}/{total} bytes ({pct}%)");
                    io::stdout().flush()?;
                }
            }
        }
        println!();
    }

    // Extract to a temp directory
    fs::create_dir_all(&tmp_extract).context("Failed to create extract directory")?;
    extract_tar_gz(&tmp_archive, &tmp_extract)?;
    fs::remove_file(&tmp_archive).ok();

    // The archive extracts to a single nested directory,
    // e.g. rust-1.87.0-x86_64-unknown-linux-gnu/
    let nested = find_single_dir(&tmp_extract)?;

    // Run the bundled install.sh to properly merge all toolchain components
    // (rustc/, cargo/, rust-std-*/, etc.) into dest_dir with correct sysroot paths.
    run_installer(&nested, &dest_dir)?;
    fs::remove_dir_all(&tmp_extract).ok();

    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive).context("Failed to open tar.gz")?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dest).context("Failed to extract tar.gz")?;
    Ok(())
}

/// Return the single subdirectory inside `dir`, or an error if there are zero or many.
fn find_single_dir(dir: &Path) -> Result<std::path::PathBuf> {
    let dirs: Vec<_> = fs::read_dir(dir)
        .context("Failed to read extract directory")?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    anyhow::ensure!(
        dirs.len() == 1,
        "Expected a single top-level directory in the archive, found {}",
        dirs.len()
    );
    Ok(dirs[0].path())
}

/// Run the Rust distribution's `install.sh` with a custom prefix.
///
/// This properly merges all toolchain components (`rustc/`, `cargo/`,
/// `rust-std-*/`, etc.) into a single directory so that `rustc` can locate
/// its sysroot and standard library at runtime. Docs are skipped to save space.
fn run_installer(src_dir: &Path, prefix: &Path) -> Result<()> {
    let script = src_dir.join("install.sh");
    anyhow::ensure!(
        script.exists(),
        "install.sh not found in extracted archive: {}",
        script.display()
    );

    println!("  Installing toolchain components...");
    let status = Command::new("sh")
        .arg(&script)
        .arg(format!("--prefix={}", prefix.display()))
        .arg("--disable-ldconfig")
        .arg("--without=rust-docs,rust-docs-json")
        .status()
        .context("Failed to run Rust install script")?;

    anyhow::ensure!(
        status.success(),
        "Rust install script exited with code: {}",
        status.code().unwrap_or(-1)
    );
    Ok(())
}

/// Remove a cached version, or prompt for interactive selection if no version is given.
pub fn remove_version(version: Option<String>) -> Result<()> {
    if let Some(v) = version {
        if cache::is_cached(&v) {
            cache::remove(&v)?;
        } else if is_sha_input(&v) {
            if let Some(matched) = cache::find_by_sha(&v) {
                cache::remove(&matched)?;
            } else {
                println!("Version '{v}' is not cached.");
            }
        } else if let Some(matched) = cache::find_by_version_prefix(&v) {
            cache::remove(&matched)?;
        } else {
            println!("Version '{v}' is not cached.");
        }
        return Ok(());
    }
    let versions = cache::cached_versions()?;
    if versions.is_empty() {
        println!("No cached versions to remove.");
        return Ok(());
    }
    let active = symlink::active_version();
    let items: Vec<String> = versions
        .iter()
        .map(|v| {
            if Some(v.as_str()) == active.as_deref() {
                format!("{v}  (active)")
            } else {
                v.clone()
            }
        })
        .collect();
    let idx = dialoguer::Select::new()
        .with_prompt("Select a version to remove")
        .items(&items)
        .interact()?;
    cache::remove(&versions[idx])?;
    Ok(())
}

const GITHUB_REPO: &str = "THernandez03/r";

fn self_artifact() -> String {
    let name = env!("CARGO_PKG_NAME");
    let os_arch = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "darwin-x64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "darwin-arm64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x64"
    } else {
        "linux-x64"
    };
    if cfg!(target_os = "windows") {
        format!("{name}-{os_arch}.exe")
    } else {
        format!("{name}-{os_arch}")
    }
}

fn strip_version_tag<'a>(tag: &'a str, name: &str) -> &'a str {
    tag.trim_start_matches(&format!("{name}-v"))
        .trim_start_matches('v')
}

/// Self-update this version manager binary to the latest GitHub release.
pub fn update_self() -> Result<()> {
    let name = env!("CARGO_PKG_NAME");
    println!("{} Checking for {} updates...", style("◆").cyan(), name);
    let client = reqwest::blocking::Client::new();
    let release: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        ))
        .header("User-Agent", format!("{name}-version-manager"))
        .send()
        .context("Failed to fetch latest release info")?
        .json()
        .context("Failed to parse release JSON")?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No tag_name in GitHub release response"))?;
    let current = env!("CARGO_PKG_VERSION");
    let remote = strip_version_tag(tag, name);
    if remote == current {
        println!(
            "{} {} is already up to date ({})",
            style("✓").green().bold(),
            name,
            style(current).cyan().bold()
        );
        return Ok(());
    }
    println!(
        "{} Updating {} {} → {}...",
        style("⬇").cyan(),
        name,
        style(current).dim(),
        style(remote).cyan().bold()
    );
    let artifact = self_artifact();
    let url = format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{artifact}");
    let exe = std::env::current_exe().context("Failed to locate current executable")?;
    let tmp = exe.with_extension("update-tmp");
    {
        let mut resp = client
            .get(&url)
            .header("User-Agent", format!("{name}-version-manager"))
            .send()
            .context("Failed to download update")?;
        if !resp.status().is_success() {
            anyhow::bail!("Download failed: HTTP {} for {}", resp.status(), url);
        }
        let file = fs::File::create(&tmp).context("Failed to create temp file for update")?;
        let mut writer = BufWriter::new(file);
        let mut buf = vec![0u8; 65536];
        loop {
            let n = resp.read(&mut buf).context("Read error during download")?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .context("Write error during download")?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
            .context("Failed to set executable permission")?;
    }
    fs::rename(&tmp, &exe).context("Failed to replace current binary")?;
    println!(
        "{} {} updated to {}.",
        style("✓").green().bold(),
        name,
        style(remote).cyan().bold()
    );
    Ok(())
}

/// Uninstall this version manager completely (removes cache, prefix directory, and the binary).
pub fn uninstall_self(yes: bool) -> Result<()> {
    let name = env!("CARGO_PKG_NAME");
    if !yes {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!(
                "This will remove all cached versions and the {name} binary. Continue?"
            ))
            .default(false)
            .interact()?;
        if !confirmed {
            println!("{}", style("Aborted.").yellow());
            return Ok(());
        }
    }
    println!("Uninstalling {}...", style(name).cyan().bold());
    let prefix = symlink::prefix();
    if prefix.exists() {
        fs::remove_dir_all(&prefix)
            .with_context(|| format!("Failed to remove {}", prefix.display()))?;
        println!("  {} Removed {}", style("✓").green(), prefix.display());
    }
    let exe = std::env::current_exe().context("Failed to locate current executable")?;
    fs::remove_file(&exe).with_context(|| format!("Failed to remove {}", exe.display()))?;
    println!("  {} Removed {}", style("✓").green(), exe.display());
    println!();
    println!(
        "{} {} uninstalled. Remove {} from your PATH if needed.",
        style("✓").green().bold(),
        name,
        exe.parent()
            .map_or_else(String::new, |p| p.display().to_string())
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_dirs<F: FnOnce(&std::path::Path, &std::path::Path)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let cache = tempfile::tempdir().expect("tempdir");
        let prefix = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", cache.path());
        std::env::set_var("R_PREFIX", prefix.path());
        f(cache.path(), prefix.path());
        std::env::remove_var("R_CACHE_DIR");
        std::env::remove_var("R_PREFIX");
    }

    /// Build a minimal in-memory tar.gz containing one file at the given path.
    fn make_tar_gz_bytes(entry_path: &str, content: &[u8]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let gz = flate2::write::GzEncoder::new(cursor, flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);

        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, entry_path, content)
            .unwrap();

        let gz = builder.into_inner().unwrap();
        gz.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_tar_gz_extracts_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tar_bytes = make_tar_gz_bytes("nested/file.txt", b"hello");
        let archive_path = tmp.path().join("archive.tar.gz");
        fs::write(&archive_path, &tar_bytes).unwrap();

        let dest = tmp.path().join("extracted");
        fs::create_dir_all(&dest).unwrap();
        extract_tar_gz(&archive_path, &dest).unwrap();

        assert!(dest.join("nested").join("file.txt").exists());
    }

    #[test]
    fn find_single_dir_returns_the_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("rust-1.87.0-x86_64-unknown-linux-gnu")).unwrap();
        let result = find_single_dir(tmp.path()).unwrap();
        assert_eq!(
            result.file_name().unwrap(),
            "rust-1.87.0-x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn find_single_dir_errors_on_multiple() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join("a")).unwrap();
        fs::create_dir_all(tmp.path().join("b")).unwrap();
        assert!(find_single_dir(tmp.path()).is_err());
    }

    #[test]
    fn find_single_dir_errors_on_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(find_single_dir(tmp.path()).is_err());
    }

    #[test]
    fn download_only_skips_if_already_cached() {
        with_temp_dirs(|cache, _prefix| {
            let bin_dir = cache.join("1.87.0").join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::write(bin_dir.join("rustc"), b"fake").unwrap();
            let result = download_only("1.87.0");
            assert!(
                result.is_ok(),
                "should skip download when cached: {result:?}"
            );
        });
    }

    // ── is_alias ───────────────────────────────────────────────────

    #[test]
    fn is_alias_known_aliases() {
        assert!(is_alias("lts"));
        assert!(is_alias("stable"));
        assert!(is_alias("current"));
        assert!(is_alias("latest"));
        assert!(is_alias("canary"));
        assert!(is_alias("nightly"));
        assert!(is_alias("next"));
        assert!(is_alias("edge"));
        assert!(is_alias("beta"));
    }

    #[test]
    fn is_alias_version_not_alias() {
        assert!(!is_alias("1.87.0"));
        assert!(!is_alias("17067e9ac"));
        assert!(!is_alias(""));
    }

    // ── looks_like_version ──────────────────────────────────────────

    #[test]
    fn looks_like_version_semver() {
        assert!(looks_like_version("1.87.0"));
        assert!(looks_like_version("1.86.0"));
    }

    #[test]
    fn looks_like_version_x_notation() {
        assert!(looks_like_version("1.x"));
        assert!(looks_like_version("1.87.X"));
    }

    #[test]
    fn looks_like_version_non_versions() {
        assert!(!looks_like_version("stable"));
        assert!(!looks_like_version("v1.2.3"));
        assert!(!looks_like_version("17067e9ac"));
    }

    // ── is_sha_input ───────────────────────────────────────────────

    #[test]
    fn is_sha_input_valid() {
        assert!(is_sha_input("17067e9ac"));
        assert!(is_sha_input("abc1234def5678"));
    }

    #[test]
    fn is_sha_input_too_short() {
        assert!(!is_sha_input("abc123"));
    }

    #[test]
    fn is_sha_input_all_digits_rejected() {
        assert!(!is_sha_input("12345678"));
    }

    #[test]
    fn is_sha_input_non_hex_rejected() {
        assert!(!is_sha_input("abc1234g"));
    }

    // ── extract_ver_sha ─────────────────────────────────────────────

    #[test]
    fn extract_ver_sha_with_sha() {
        let (ver, sha) = extract_ver_sha("1.87.0+17067e9ac");
        assert_eq!(ver, "1.87.0");
        assert_eq!(sha, Some("17067e9ac"));
    }

    #[test]
    fn extract_ver_sha_without_sha() {
        let (ver, sha) = extract_ver_sha("1.87.0");
        assert_eq!(ver, "1.87.0");
        assert!(sha.is_none());
    }

    #[test]
    fn extract_ver_sha_strips_nightly_suffix() {
        let (ver, sha) = extract_ver_sha("1.90.0-nightly+17067e9ac");
        assert_eq!(ver, "1.90.0");
        assert_eq!(sha, Some("17067e9ac"));
    }

    #[test]
    fn extract_ver_sha_nightly_no_sha() {
        let (ver, sha) = extract_ver_sha("1.90.0-nightly");
        assert_eq!(ver, "1.90.0");
        assert!(sha.is_none());
    }

    #[test]
    fn extract_ver_sha_strips_beta_suffix() {
        let (ver, sha) = extract_ver_sha("1.90.0-beta.3+17067e9ac");
        assert_eq!(ver, "1.90.0");
        assert_eq!(sha, Some("17067e9ac"));
    }

    // ── strip_version_tag ──────────────────────────────────────────

    #[test]
    fn strip_version_tag_strips_name_prefix() {
        assert_eq!(strip_version_tag("r-v0.5.0", "r"), "0.5.0");
    }

    #[test]
    fn strip_version_tag_strips_bare_v_prefix() {
        assert_eq!(strip_version_tag("v0.5.0", "r"), "0.5.0");
    }

    #[test]
    fn strip_version_tag_bare_version_unchanged() {
        assert_eq!(strip_version_tag("0.5.0", "r"), "0.5.0");
    }
}
