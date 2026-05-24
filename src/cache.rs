use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// Root cache directory: `$R_CACHE_DIR` or `$R_PREFIX/versions`, defaulting to `~/.r/versions`.
pub fn cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("R_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    crate::symlink::prefix().join("versions")
}

/// Path to the directory for a specific cached version.
pub fn version_dir(tag: &str) -> PathBuf {
    cache_dir().join(tag)
}

/// Path to the `rustc` binary inside a cached version's `bin/` directory.
pub fn rustc_binary(tag: &str) -> PathBuf {
    let dir = version_dir(tag).join("bin");
    #[cfg(target_os = "windows")]
    return dir.join("rustc.exe");
    #[cfg(not(target_os = "windows"))]
    return dir.join("rustc");
}

/// Returns `true` if the version is already installed in the cache.
pub fn is_cached(tag: &str) -> bool {
    rustc_binary(tag).exists()
}

/// Returns the path to the `rustc` binary, or an error if not cached.
pub fn which(tag: &str) -> Result<PathBuf> {
    let path = rustc_binary(tag);
    if path.exists() {
        Ok(path)
    } else {
        anyhow::bail!("Version '{tag}' is not cached. Run `r {tag}` to install it.")
    }
}

/// Remove a cached version directory.
pub fn remove(tag: &str) -> Result<()> {
    let dir = version_dir(tag);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("Failed to remove cached version '{tag}'"))?;
        println!("Removed {tag}");
    } else {
        println!("Version '{tag}' is not cached.");
    }
    Ok(())
}

/// Remove all cached versions except the currently active one.
/// Remove all cached versions except the currently active one.
/// When `force` is `true`, all versions including the active one are removed.
pub fn prune(force: bool) -> Result<()> {
    let active = crate::symlink::active_version();
    let dir = cache_dir();

    if !dir.exists() {
        println!("Cache directory does not exist.");
        return Ok(());
    }

    for entry in fs::read_dir(&dir).context("Failed to read cache directory")? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap_or_default();
        if !force && Some(&name) == active.as_ref() {
            println!("Skipped {name} (active — use --force to remove)");
            continue;
        }
        if entry.path().is_dir() {
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("Failed to remove '{name}'"))?;
            println!("Removed {name}");
        }
    }
    Ok(())
}

/// Returns `true` if `a` is a prefix of `b` or `b` is a prefix of `a`.
/// Used for fuzzy SHA matching between stored (short) and user-provided (long) SHAs.
pub fn sha_matches(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Returns the SHA portion of a cache key (the part after `+`), if any.
pub fn cache_key_sha(key: &str) -> Option<&str> {
    key.split_once('+').map(|(_, sha)| sha)
}

/// Find a cached version matching the given version prefix.
///
/// A match occurs when the cache-directory name equals `prefix` exactly, starts
/// with `"{prefix}+"` (exact version with SHA), or starts with `"{prefix}."`
/// (partial version, e.g. `"1.87"` matches `"1.87.0+abc"`).
/// If multiple entries match, the most recently modified is returned.
pub fn find_by_version_prefix(prefix: &str) -> Option<String> {
    let dir = cache_dir();
    if !dir.exists() {
        return None;
    }
    let prefix_plus = format!("{prefix}+");
    let prefix_dot = format!("{prefix}.");
    let mut best: Option<(std::time::SystemTime, String)> = None;
    let Ok(entries) = fs::read_dir(&dir) else {
        return None;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if (name == prefix || name.starts_with(&prefix_plus) || name.starts_with(&prefix_dot))
            && rustc_binary(&name).exists()
        {
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let is_newer = best.as_ref().map_or(true, |(t, _)| mtime > *t);
            if is_newer {
                best = Some((mtime, name));
            }
        }
    }
    best.map(|(_, name)| name)
}

/// Find a cached version whose SHA component fuzzy-matches the given SHA.
///
/// The SHA component is the part after `+` in the cache key.
/// If multiple entries match, the most recently modified is returned.
pub fn find_by_sha(sha: &str) -> Option<String> {
    let dir = cache_dir();
    if !dir.exists() {
        return None;
    }
    let mut best: Option<(std::time::SystemTime, String)> = None;
    let Ok(entries) = fs::read_dir(&dir) else {
        return None;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(cached_sha) = cache_key_sha(&name) {
            if sha_matches(cached_sha, sha) && rustc_binary(&name).exists() {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let is_newer = best.as_ref().map_or(true, |(t, _)| mtime > *t);
                if is_newer {
                    best = Some((mtime, name));
                }
            }
        }
    }
    best.map(|(_, name)| name)
}

/// Rename a cached version directory from `old_key` to `new_key`.
/// No-op if `old_key` does not exist or `new_key` already exists.
pub fn rename_version(old_key: &str, new_key: &str) -> Result<()> {
    let old_dir = version_dir(old_key);
    let new_dir = version_dir(new_key);
    if old_dir.exists() && !new_dir.exists() {
        fs::rename(&old_dir, &new_dir).with_context(|| {
            format!("Failed to rename cache entry '{old_key}' \u{2192} '{new_key}'")
        })?;
    }
    Ok(())
}

/// Return all locally cached version tags, newest first.
pub fn cached_versions() -> Result<Vec<String>> {
    let dir = cache_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut versions = vec![];
    for entry in fs::read_dir(&dir).context("Failed to read cache directory")? {
        let entry = entry?;
        if entry.path().is_dir() {
            let name = entry.file_name().into_string().unwrap_or_default();
            if !name.is_empty() {
                versions.push(name);
            }
        }
    }
    versions.sort_by(|a, b| b.cmp(a));
    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_cache<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", dir.path());
        f(dir.path());
        std::env::remove_var("R_CACHE_DIR");
    }

    #[test]
    fn cache_dir_respects_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", dir.path());
        let result = cache_dir();
        std::env::remove_var("R_CACHE_DIR");
        assert_eq!(result, dir.path());
    }

    #[test]
    fn version_dir_is_under_cache_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", dir.path());
        let vdir = version_dir("1.87.0");
        std::env::remove_var("R_CACHE_DIR");
        assert_eq!(vdir, dir.path().join("1.87.0"));
    }

    #[test]
    fn rustc_binary_inside_bin_subdir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", dir.path());
        let bin = rustc_binary("1.87.0");
        std::env::remove_var("R_CACHE_DIR");
        assert!(bin.starts_with(dir.path()));
        let name = bin.file_name().unwrap().to_string_lossy();
        assert!(name == "rustc" || name == "rustc.exe");
    }

    #[test]
    fn is_cached_returns_false_when_missing() {
        with_temp_cache(|_| {
            assert!(!is_cached("99.0.0"));
        });
    }

    #[test]
    fn is_cached_returns_true_when_binary_exists() {
        with_temp_cache(|base| {
            let bin_dir = base.join("1.87.0").join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::write(bin_dir.join("rustc"), b"fake").unwrap();
            assert!(is_cached("1.87.0"));
        });
    }

    #[test]
    fn which_errors_when_not_cached() {
        with_temp_cache(|_| {
            assert!(which("99.0.0").is_err());
        });
    }

    #[test]
    fn which_returns_path_when_cached() {
        with_temp_cache(|base| {
            let bin_dir = base.join("1.87.0").join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            fs::write(bin_dir.join("rustc"), b"fake").unwrap();
            assert!(which("1.87.0").is_ok());
        });
    }

    #[test]
    fn remove_deletes_version_dir() {
        with_temp_cache(|base| {
            let vdir = base.join("1.87.0");
            fs::create_dir_all(&vdir).unwrap();
            remove("1.87.0").unwrap();
            assert!(!vdir.exists());
        });
    }

    #[test]
    fn remove_is_ok_when_not_cached() {
        with_temp_cache(|_| {
            assert!(remove("99.0.0").is_ok());
        });
    }

    #[test]
    fn cached_versions_empty_when_dir_missing() {
        with_temp_cache(|base| {
            fs::remove_dir_all(base).unwrap();
            assert_eq!(cached_versions().unwrap(), Vec::<String>::new());
        });
    }

    #[test]
    fn cached_versions_sorted_desc() {
        with_temp_cache(|base| {
            fs::create_dir_all(base.join("1.85.0")).unwrap();
            fs::create_dir_all(base.join("1.87.0")).unwrap();
            fs::create_dir_all(base.join("1.86.0")).unwrap();
            let versions = cached_versions().unwrap();
            assert_eq!(versions[0], "1.87.0");
        });
    }

    #[test]
    fn prune_removes_inactive_versions() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cache = tempfile::tempdir().expect("tempdir");
        let prefix = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", cache.path());
        std::env::set_var("R_PREFIX", prefix.path());

        fs::write(prefix.path().join(".active"), "1.87.0").unwrap();
        fs::create_dir_all(cache.path().join("1.87.0")).unwrap();
        fs::create_dir_all(cache.path().join("1.86.0")).unwrap();

        prune(false).unwrap();

        assert!(cache.path().join("1.87.0").exists());
        assert!(!cache.path().join("1.86.0").exists());

        std::env::remove_var("R_CACHE_DIR");
        std::env::remove_var("R_PREFIX");
    }

    #[test]
    fn prune_force_removes_all_versions() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cache = tempfile::tempdir().expect("tempdir");
        let prefix = tempfile::tempdir().expect("tempdir");
        std::env::set_var("R_CACHE_DIR", cache.path());
        std::env::set_var("R_PREFIX", prefix.path());

        fs::write(prefix.path().join(".active"), "1.87.0").unwrap();
        fs::create_dir_all(cache.path().join("1.87.0")).unwrap();
        fs::create_dir_all(cache.path().join("1.85.0")).unwrap();

        prune(true).unwrap();

        assert!(
            !cache.path().join("1.87.0").exists(),
            "--force should remove active"
        );
        assert!(
            !cache.path().join("1.85.0").exists(),
            "--force should remove inactive"
        );

        std::env::remove_var("R_CACHE_DIR");
        std::env::remove_var("R_PREFIX");
    }

    fn make_cached_rustc(tag: &str) {
        let path = rustc_binary(tag);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"fake").unwrap();
    }

    // ── sha_matches ───────────────────────────────────────────────────

    #[test]
    fn sha_matches_identical() {
        assert!(sha_matches("abc1234def", "abc1234def"));
    }

    #[test]
    fn sha_matches_a_prefix_of_b() {
        assert!(sha_matches("abc1234", "abc1234def5678"));
    }

    #[test]
    fn sha_matches_b_prefix_of_a() {
        assert!(sha_matches("abc1234def5678", "abc1234"));
    }

    #[test]
    fn sha_matches_unrelated_returns_false() {
        assert!(!sha_matches("abc1234", "def5678"));
    }

    // ── cache_key_sha ─────────────────────────────────────────────────

    #[test]
    fn cache_key_sha_present() {
        assert_eq!(cache_key_sha("1.87.0+17067e9ac"), Some("17067e9ac"));
    }

    #[test]
    fn cache_key_sha_absent() {
        assert!(cache_key_sha("1.87.0").is_none());
    }

    // ── find_by_version_prefix ────────────────────────────────────────

    #[test]
    fn find_by_version_prefix_exact() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0");
            assert_eq!(find_by_version_prefix("1.87.0"), Some("1.87.0".to_string()));
        });
    }

    #[test]
    fn find_by_version_prefix_with_sha_suffix() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0+17067e9ac");
            assert_eq!(
                find_by_version_prefix("1.87.0"),
                Some("1.87.0+17067e9ac".to_string())
            );
        });
    }

    #[test]
    fn find_by_version_prefix_dot_match() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0");
            assert_eq!(find_by_version_prefix("1.87"), Some("1.87.0".to_string()));
        });
    }

    #[test]
    fn find_by_version_prefix_no_match() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0");
            assert!(find_by_version_prefix("1.86.0").is_none());
        });
    }

    #[test]
    fn find_by_version_prefix_requires_binary() {
        with_temp_cache(|base| {
            fs::create_dir_all(base.join("1.87.0")).unwrap();
            assert!(find_by_version_prefix("1.87.0").is_none());
        });
    }

    // ── find_by_sha ───────────────────────────────────────────────────

    #[test]
    fn find_by_sha_exact() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0+17067e9ac");
            assert_eq!(
                find_by_sha("17067e9ac"),
                Some("1.87.0+17067e9ac".to_string())
            );
        });
    }

    #[test]
    fn find_by_sha_input_prefix_of_stored() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0+17067e9ac123456");
            assert_eq!(
                find_by_sha("17067e9a"),
                Some("1.87.0+17067e9ac123456".to_string())
            );
        });
    }

    #[test]
    fn find_by_sha_stored_prefix_of_input() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0+17067e9a");
            assert_eq!(
                find_by_sha("17067e9ac123456"),
                Some("1.87.0+17067e9a".to_string())
            );
        });
    }

    #[test]
    fn find_by_sha_no_match() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0+17067e9ac");
            assert!(find_by_sha("abc99999").is_none());
        });
    }

    #[test]
    fn find_by_sha_ignores_entry_without_sha() {
        with_temp_cache(|_| {
            make_cached_rustc("1.87.0");
            assert!(find_by_sha("187000").is_none());
        });
    }

    // ── rename_version ────────────────────────────────────────────────

    #[test]
    fn rename_version_moves_dir() {
        with_temp_cache(|base| {
            fs::create_dir_all(base.join("1.87.0")).unwrap();
            rename_version("1.87.0", "1.87.0+17067e9ac").unwrap();
            assert!(!base.join("1.87.0").exists());
            assert!(base.join("1.87.0+17067e9ac").exists());
        });
    }

    #[test]
    fn rename_version_noop_when_old_missing() {
        with_temp_cache(|_| {
            assert!(rename_version("nonexistent", "also-nonexistent").is_ok());
        });
    }

    #[test]
    fn rename_version_noop_when_new_exists() {
        with_temp_cache(|base| {
            fs::create_dir_all(base.join("1.87.0")).unwrap();
            fs::create_dir_all(base.join("1.87.0+17067e9ac")).unwrap();
            rename_version("1.87.0", "1.87.0+17067e9ac").unwrap();
            assert!(base.join("1.87.0").exists());
            assert!(base.join("1.87.0+17067e9ac").exists());
        });
    }
}
