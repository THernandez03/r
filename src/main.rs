mod arch;
mod cache;
mod install;
mod list;
mod releases;
mod symlink;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// r — Interactively manage your Rust versions
#[derive(Parser)]
#[command(name = "r", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Version to install (e.g. 1.87.0, stable, beta, nightly)
    version: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a Rust toolchain version
    Install {
        /// Version string (e.g. 1.87.0, v1.87.0, stable, beta, nightly)
        version: String,
    },
    /// List locally cached versions
    Ls,
    /// List recent remote releases
    LsRemote,
    /// Remove one or more cached versions
    Rm {
        /// Versions to remove
        versions: Vec<String>,
    },
    /// Remove all cached versions except the active one
    Prune,
    /// Show path to the rustc binary for a cached version
    Which {
        /// Version to look up
        version: String,
    },
    /// Run the rustc binary of a specific cached version
    Run {
        /// Version string
        version: String,
        /// Arguments to pass to rustc
        args: Vec<String>,
    },
    /// Download a version into cache without activating it
    Download {
        /// Version string
        version: String,
    },
    /// Show diagnostic information
    Doctor,
    /// Remove the active Rust toolchain symlink (does not remove cache)
    Uninstall,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if let Some(version) = cli.version {
                install::install(&version)?;
            } else {
                list::interactive_picker()?;
            }
        }
        Some(Commands::Install { version }) => install::install(&version)?,
        Some(Commands::Ls) => list::list_local()?,
        Some(Commands::LsRemote) => releases::list_remote()?,
        Some(Commands::Rm { versions }) => {
            for v in &versions {
                cache::remove(v)?;
            }
        }
        Some(Commands::Prune) => cache::prune()?,
        Some(Commands::Which { version }) => {
            let path = cache::which(&version)?;
            println!("{}", path.display());
        }
        Some(Commands::Run { version, args }) => install::run(&version, &args)?,
        Some(Commands::Download { version }) => install::download_only(&version)?,
        Some(Commands::Doctor) => diagnostics::doctor(),
        Some(Commands::Uninstall) => symlink::uninstall(),
    }

    Ok(())
}

mod diagnostics {
    use crate::{cache, symlink};

    pub fn doctor() {
        println!("r — Rust version manager diagnostics");
        println!();
        let prefix = symlink::prefix();
        println!("  install prefix : {}", prefix.display());
        let cache_dir = cache::cache_dir();
        println!("  cache dir      : {}", cache_dir.display());
        let active = symlink::active_version();
        match active {
            Some(v) => println!("  active version : {v}"),
            None => println!("  active version : (none)"),
        }
    }
}
