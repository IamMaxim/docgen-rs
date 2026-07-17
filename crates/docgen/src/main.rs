use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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
    /// Lint the site's docs: link integrity, diagrams, assets, metadata,
    /// structure. Exit 0 = clean, 1 = error-level findings, 2 = lint failure.
    Lint {
        /// Project root (defaults to the current directory).
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = LintFormat::Pretty)]
        format: LintFormat,
        /// Promote every warning to an error (affects the exit code).
        #[arg(long, default_value_t = false)]
        deny_warnings: bool,
        /// Run only these rule ids (comma-separated, may repeat).
        #[arg(long, value_delimiter = ',')]
        rules: Vec<String>,
        /// Pretty output shows error-level findings only (the summary line
        /// keeps the true counts). Other formats are unaffected.
        #[arg(long, default_value_t = false)]
        quiet: bool,
        /// List every rule (id, default severity, description) and exit.
        #[arg(long, default_value_t = false)]
        list_rules: bool,
    },
    /// Run an ephemeral PlantUML server in a container for build-time diagram
    /// rendering. Runs in the foreground; press Ctrl-C to stop (auto-removed).
    Plantuml {
        /// Host port to publish the server on (the default docgen connects to).
        #[arg(long, default_value_t = 8080)]
        port: u16,
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
        Command::Lint {
            root,
            format,
            deny_warnings,
            rules,
            quiet,
            list_rules,
        } => run_lint(&root, format, deny_warnings, rules, quiet, list_rules),
        Command::Plantuml { port } => docgen_plantuml::run_container(port),
    }
}

/// Output format for `docgen lint`.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LintFormat {
    /// Human-readable terminal output (colored on a TTY).
    Pretty,
    /// Machine JSON: `{"diagnostics": [...], "summary": {...}}`.
    Json,
    /// GitHub Actions workflow-command annotations.
    Github,
    /// GitLab Code Quality report.
    Gitlab,
}

/// Run `docgen lint`. Exit codes: 0 = no error-level findings, 1 = at least
/// one error-level finding, 2 = the lint run itself failed.
fn run_lint(
    root: &std::path::Path,
    format: LintFormat,
    deny_warnings: bool,
    rules: Vec<String>,
    quiet: bool,
    list_rules: bool,
) -> Result<()> {
    if list_rules {
        for (id, severity, description) in docgen_lint::list_rules() {
            // `to_string` first: Severity's Display ignores width padding.
            println!("{id:<22} {:<6} {description}", severity.to_string());
        }
        return Ok(());
    }

    let options = docgen_lint::LintOptions {
        only_rules: (!rules.is_empty()).then_some(rules),
        deny_warnings,
    };
    let outcome = match docgen_lint::run(root, &options) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    let rendered = match format {
        LintFormat::Pretty => {
            let use_color = std::io::stdout().is_terminal();
            if quiet {
                // Hide warn/info findings but keep the true summary counts.
                let errors_only = docgen_lint::LintOutcome {
                    diagnostics: outcome
                        .diagnostics
                        .iter()
                        .filter(|d| d.severity == docgen_lint::Severity::Error)
                        .cloned()
                        .collect(),
                    ..outcome.clone()
                };
                docgen_lint::format::pretty(&errors_only, use_color)
            } else {
                docgen_lint::format::pretty(&outcome, use_color)
            }
        }
        LintFormat::Json => docgen_lint::format::json(&outcome),
        LintFormat::Github => docgen_lint::format::github(&outcome),
        LintFormat::Gitlab => docgen_lint::format::gitlab(&outcome),
    };
    print!("{rendered}");
    if !rendered.ends_with('\n') {
        println!();
    }

    if outcome.errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}
