use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "docgen",
    version,
    about = "Static documentation-site generator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the static site from `docs/` into `dist/`.
    Build {
        /// Project root (defaults to the current directory).
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Scaffold a new docgen site (replaces create-docgen).
    Init {
        /// Target directory (defaults to the current directory).
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Scaffold even if the target dir is non-empty.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Run the dev server with live reload + in-browser editor (localhost only).
    Dev {
        /// Project root (defaults to the current directory).
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Loopback port to bind.
        #[arg(long, default_value_t = 4321)]
        port: u16,
        /// Open a browser on start.
        #[arg(long, default_value_t = false)]
        open: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { root } => {
            let outcome = docgen_build::build(&root)?;
            println!(
                "Built {} page(s) -> {}",
                outcome.page_count,
                outcome.out_dir.display()
            );
            Ok(())
        }
        Command::Init { dir, force } => {
            docgen_init::scaffold(&docgen_init::InitOptions {
                target: dir.clone(),
                force,
            })?;
            println!("Scaffolded a new docgen site at {}", dir.display());
            println!("Next: cd {} && docgen dev", dir.display());
            Ok(())
        }
        Command::Dev { root, port, open } => docgen_server::serve(docgen_server::DevOptions {
            project_root: root,
            port,
            open,
        }),
    }
}
