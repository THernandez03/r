use anyhow::{Context, Result};
use std::fs;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::process::Command;

use console::style;

use crate::{arch, cache, releases, symlink};

/// Install a Rust toolchain and activate it.
pub fn install(version_str: &str) -> Result<()> {
    let tag = releases::resolve_tag(version_str)?;

    if symlink::active_version().as_deref() == Some(&tag) {
        println!(
            "{} Rust {} is already the active version.",
            style("✓").green().bold(),
            style(&tag).cyan().bold(),
        );
        return Ok(());
    }

    if cache::is_cached(&tag) {
        println!(
            "{} Rust {} is already cached.",
            style("◆").dim(),
            style(&tag).cyan(),
        );
    } else {
        println!(
            "{} Downloading Rust {}...",
            style("⬇").cyan(),
            style(&tag).cyan().bold(),
        );
        let url = arch::download_url(&tag);
        download_version(&url, &tag)?;
    }

    let from = symlink::active_version();
    match &from {
        Some(f) => println!(
            "{} Activating Rust {} \u{2192} {}...",
            style("◆").magenta(),
            style(f).cyan().bold(),
            style(&tag).cyan().bold(),
        ),
        None => println!(
            "{} Activating Rust {}...",
            style("◆").magenta(),
            style(&tag).cyan().bold(),
        ),
    }
    symlink::activate(&tag)?;
    println!(
        "{} Installed Rust {} successfully.",
        style("✓").green().bold(),
        style(&tag).cyan().bold(),
    );
    Ok(())
}

/// Download a version into cache without activating it.
pub fn download_only(version_str: &str) -> Result<()> {
    let tag = releases::resolve_tag(version_str)?;
    if cache::is_cached(&tag) {
        println!("Version {tag} is already cached.");
        return Ok(());
    }
    println!("Downloading Rust {tag}...");
    let url = arch::download_url(&tag);
    download_version(&url, &tag)
}

/// Run a specific cached Rust version's `rustc` with given arguments.
pub fn run(version_str: &str, args: &[String]) -> Result<()> {
    let tag = releases::resolve_tag(version_str)?;

    if !cache::is_cached(&tag) {
        println!("Version {tag} is not cached. Downloading...");
        let url = arch::download_url(&tag);
        download_version(&url, &tag)?;
    }

    let binary = cache::rustc_binary(&tag);
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
        .filter_map(|e| e.ok())
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
            assert!(result.is_ok(), "should skip download when cached: {result:?}");
        });
    }
}
