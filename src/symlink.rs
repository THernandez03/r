use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Installation prefix: `$R_PREFIX` or `~/.r`.
pub fn prefix() -> PathBuf {
    if let Ok(p) = std::env::var("R_PREFIX") {
        return PathBuf::from(p);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".r")
}

/// The bin symlink directory: `{prefix}/bin`.
pub fn bin_dir() -> PathBuf {
    prefix().join("bin")
}

fn remove_bin_symlink(bin: &std::path::Path) {
    if bin.symlink_metadata().is_ok() {
        #[cfg(unix)]
        {
            fs::remove_file(bin).ok();
        }
        #[cfg(windows)]
        {
            fs::remove_dir(bin).ok();
        }
    }
}

/// Activate a cached toolchain by pointing `~/.r/bin` at the cached version's
/// `bin/` subdirectory, exposing `rustc`, `cargo`, `rustfmt`, `clippy`,
/// `rustdoc`, and all other toolchain binaries through the symlink.
pub fn activate(tag: &str) -> Result<()> {
    let bin = bin_dir();
    let cached_bin = crate::cache::version_dir(tag).join("bin");

    anyhow::ensure!(
        cached_bin.is_dir(),
        "Cached bin directory not found: {}",
        cached_bin.display()
    );

    if let Some(parent) = bin.parent() {
        fs::create_dir_all(parent).context("Failed to create prefix directory")?;
    }

    remove_bin_symlink(&bin);

    #[cfg(unix)]
    std::os::unix::fs::symlink(&cached_bin, &bin).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            bin.display(),
            cached_bin.display()
        )
    })?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&cached_bin, &bin).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            bin.display(),
            cached_bin.display()
        )
    })?;

    let marker = prefix().join(".active");
    fs::write(&marker, tag).context("Failed to write active version marker")?;

    Ok(())
}

/// Read the currently active version from the marker file.
pub fn active_version() -> Option<String> {
    let marker = prefix().join(".active");
    fs::read_to_string(marker)
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_prefix<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_PREFIX", dir.path());
        std::env::remove_var("R_CACHE_DIR");
        f(dir.path());
        std::env::remove_var("R_PREFIX");
    }

    #[test]
    fn prefix_respects_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_PREFIX", dir.path());
        let result = prefix();
        std::env::remove_var("R_PREFIX");
        assert_eq!(result, dir.path());
    }

    #[test]
    fn bin_dir_is_under_prefix() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_PREFIX", dir.path());
        let b = bin_dir();
        std::env::remove_var("R_PREFIX");
        assert_eq!(b, dir.path().join("bin"));
    }

    #[test]
    fn active_version_returns_none_when_marker_missing() {
        with_temp_prefix(|_| {
            assert_eq!(active_version(), None);
        });
    }

    #[test]
    fn active_version_reads_marker_file() {
        with_temp_prefix(|base| {
            fs::write(base.join(".active"), "1.87.0").unwrap();
            assert_eq!(active_version(), Some("1.87.0".to_string()));
        });
    }

    #[test]
    fn active_version_trims_whitespace() {
        with_temp_prefix(|base| {
            fs::write(base.join(".active"), "1.87.0\n").unwrap();
            assert_eq!(active_version(), Some("1.87.0".to_string()));
        });
    }

    #[cfg(unix)]
    #[test]
    fn activate_creates_symlink_and_marker() {
        with_temp_prefix(|base| {
            std::env::set_var("R_CACHE_DIR", base.join("versions"));
            let bin_dir = base.join("versions").join("1.87.0").join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::write(bin_dir.join("rustc"), b"#!/bin/sh\necho hi").unwrap();

            activate("1.87.0").unwrap();

            let link = base.join("bin");
            assert!(link.symlink_metadata().is_ok(), "symlink should exist");
            assert_eq!(active_version(), Some("1.87.0".to_string()));
            std::env::remove_var("R_CACHE_DIR");
        });
    }

    #[cfg(unix)]
    #[test]
    fn activate_replaces_existing_symlink() {
        with_temp_prefix(|base| {
            std::env::set_var("R_CACHE_DIR", base.join("versions"));
            for v in &["1.86.0", "1.87.0"] {
                let vdir = base.join("versions").join(v).join("bin");
                fs::create_dir_all(&vdir).unwrap();
                fs::write(vdir.join("rustc"), b"#!/bin/sh").unwrap();
            }
            activate("1.86.0").unwrap();
            activate("1.87.0").unwrap();
            assert_eq!(active_version(), Some("1.87.0".to_string()));
            std::env::remove_var("R_CACHE_DIR");
        });
    }

}
