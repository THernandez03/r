/// Binary-level integration tests for the `r` CLI.
///
/// These tests invoke the compiled binary as a user would, exercising the full
/// path from CLI parsing → command dispatch → output.  All tests are offline:
/// they pre-populate a temporary cache directory with a fake binary so that
/// `fetch` and `which` return immediately without hitting the network.
use std::fs;
use std::path::Path;
use std::process::Command;

fn r() -> Command {
    Command::new(env!("CARGO_BIN_EXE_r"))
}

/// Create a fake cached rustc binary at the path `r` expects.
/// Rust binary lives at `{cache}/{version}/bin/rustc`.
fn fake_cache(dir: &Path, version: &str) {
    let bin = dir.join(version).join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("rustc"), b"#!/bin/sh\necho fake\n").unwrap();
}

// ── --help / --version ────────────────────────────────────────────────────────

#[test]
fn help_exits_zero() {
    assert!(r().arg("--help").status().unwrap().success());
}

#[test]
fn version_prints_semver() {
    let out = r().arg("--version").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.split_whitespace()
            .any(|w| w.contains('.') && w.chars().next().is_some_and(|c| c.is_ascii_digit())),
        "expected semver in version output, got: {s:?}"
    );
}

#[test]
fn unknown_flag_exits_nonzero() {
    assert!(!r().arg("--not-a-real-flag").status().unwrap().success());
}

// ── ls ────────────────────────────────────────────────────────────────────────

#[test]
fn ls_empty_cache_reports_none() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    let out = r()
        .arg("ls")
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("No cached Rust versions found."),
        "unexpected output: {stdout}"
    );
}

#[test]
fn ls_shows_cached_version() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.87.0");
    let out = r()
        .arg("ls")
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1.87.0"),
        "expected version in output: {stdout}"
    );
}

#[test]
fn ls_marks_active_version() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.87.0");
    fs::write(prefix.path().join(".active"), "1.87.0").unwrap();
    let out = r()
        .arg("ls")
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("(active)"),
        "expected (active) marker in output: {stdout}"
    );
}

// ── fetch ─────────────────────────────────────────────────────────────────────

#[test]
fn fetch_skips_download_when_already_cached() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.87.0");
    let status = r()
        .args(["fetch", "1.87.0"])
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .status()
        .unwrap();
    assert!(
        status.success(),
        "fetch should succeed without network when version is already cached"
    );
}

// ── which ─────────────────────────────────────────────────────────────────────

#[test]
fn which_prints_binary_path() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.87.0");
    let out = r()
        .args(["which", "1.87.0"])
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1.87.0") && stdout.contains("rustc"),
        "expected path containing version and 'rustc': {stdout}"
    );
}

#[test]
fn which_fails_when_not_cached() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    let status = r()
        .args(["which", "1.87.0"])
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .status()
        .unwrap();
    assert!(
        !status.success(),
        "which should fail when the version is not cached"
    );
}

// ── prune ─────────────────────────────────────────────────────────────────────

#[test]
fn prune_removes_inactive_keeps_active() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.86.0");
    fake_cache(cache.path(), "1.87.0");
    fs::write(prefix.path().join(".active"), "1.87.0").unwrap();
    r().arg("prune")
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .status()
        .unwrap();
    assert!(
        !cache.path().join("1.86.0").exists(),
        "inactive should be removed"
    );
    assert!(
        cache.path().join("1.87.0").exists(),
        "active should be kept"
    );
}

#[test]
fn prune_force_removes_all_including_active() {
    let cache = tempfile::tempdir().unwrap();
    let prefix = tempfile::tempdir().unwrap();
    fake_cache(cache.path(), "1.86.0");
    fake_cache(cache.path(), "1.87.0");
    fs::write(prefix.path().join(".active"), "1.87.0").unwrap();
    r().args(["prune", "--force"])
        .env("R_CACHE_DIR", cache.path())
        .env("R_PREFIX", prefix.path())
        .status()
        .unwrap();
    assert!(
        !cache.path().join("1.86.0").exists(),
        "inactive should be removed by --force"
    );
    assert!(
        !cache.path().join("1.87.0").exists(),
        "active should be removed by --force"
    );
}
