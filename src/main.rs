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
}

#[derive(Subcommand)]
enum Commands {
    /// List locally cached versions
    Ls,
    /// List recent remote releases
    LsRemote,
    /// Remove a cached Rust toolchain version (interactive if no version given)
    #[command(alias = "rm")]
    Remove {
        /// Version to remove (omit for interactive selection)
        version: Option<String>,
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
    /// Fetch a Rust toolchain version into cache without activating it
    Fetch {
        /// Version string
        version: String,
    },
    /// Show version manager and runtime information
    Info,
    /// Update r to the latest available version
    Update,
    /// Uninstall r completely (removes cached versions, prefix, and the r binary)
    Uninstall,
    /// Install a Rust toolchain (e.g. 1.87.0, stable, beta, nightly)
    #[command(external_subcommand)]
    Version(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => list::interactive_picker()?,
        Some(Commands::Ls) => list::list_local()?,
        Some(Commands::LsRemote) => releases::list_remote()?,
        Some(Commands::Remove { version }) => install::remove_version(version)?,
        Some(Commands::Prune) => cache::prune()?,
        Some(Commands::Which { version }) => {
            let path = cache::which(&version)?;
            println!("{}", path.display());
        }
        Some(Commands::Run { version, args }) => install::run(&version, &args)?,
        Some(Commands::Fetch { version }) => install::download_only(&version)?,
        Some(Commands::Info) => diagnostics::info(),
        Some(Commands::Update) => install::update_self()?,
        Some(Commands::Uninstall) => install::uninstall_self()?,
        Some(Commands::Version(args)) => install::install(&args[0])?,
    }

    Ok(())
}

mod diagnostics {
    use crate::{cache, symlink};

    pub fn info() {
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
