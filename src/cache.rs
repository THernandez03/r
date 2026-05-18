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
        anyhow::bail!("Version '{tag}' is not cached. Run `r install {tag}` first.")
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
pub fn prune() -> Result<()> {
    let active = crate::symlink::active_version();
    let dir = cache_dir();

    if !dir.exists() {
        println!("Cache directory does not exist.");
        return Ok(());
    }

    for entry in fs::read_dir(&dir).context("Failed to read cache directory")? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap_or_default();
        if Some(&name) == active.as_ref() {
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

        prune().unwrap();

        assert!(cache.path().join("1.87.0").exists());
        assert!(!cache.path().join("1.86.0").exists());

        std::env::remove_var("R_CACHE_DIR");
        std::env::remove_var("R_PREFIX");
    }
}
