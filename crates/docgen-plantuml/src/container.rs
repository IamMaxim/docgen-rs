//! The `docgen plantuml` ephemeral-server command: run a PlantUML server
//! container in the foreground so a local `docgen build`/`docgen dev` can render
//! diagrams. Ctrl-C stops it; `--rm` removes it.

use std::process::Command;

use anyhow::{Context, Result};

/// The container image the command runs.
pub const PLANTUML_IMAGE: &str = "plantuml/plantuml-server:jetty";
/// The port the server listens on inside the container.
const CONTAINER_PORT: u16 = 8080;

/// The container runtime to invoke: `DOCGEN_CONTAINER_RUNTIME` (e.g. `podman`) or
/// `docker` by default.
fn runtime() -> String {
    std::env::var("DOCGEN_CONTAINER_RUNTIME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "docker".to_string())
}

/// Build the container-runtime argument vector for a foreground, auto-removed
/// PlantUML server publishing `host_port` → the container's PlantUML port. Pure
/// (no process spawned) so it is unit-testable.
fn container_args(host_port: u16, image: &str) -> Vec<String> {
    vec![
        "run".to_string(),
        "--rm".to_string(),
        "-p".to_string(),
        format!("{host_port}:{CONTAINER_PORT}"),
        image.to_string(),
    ]
}

/// Run the PlantUML server container in the foreground, publishing it on
/// `host_port`. Blocks until the container exits (Ctrl-C). Returns an error if
/// the runtime binary is missing or the container exits non-zero.
pub fn run_container(host_port: u16) -> Result<()> {
    let runtime = runtime();
    let args = container_args(host_port, PLANTUML_IMAGE);
    println!(
        "Starting PlantUML server ({PLANTUML_IMAGE}) via {runtime} on \
         http://localhost:{host_port}"
    );
    println!("Point docgen at it with the default server URL, or set DOCGEN_PLANTUML_SERVER.");
    println!("Press Ctrl-C to stop.");

    let status = Command::new(&runtime)
        .args(&args)
        .status()
        .with_context(|| {
            format!(
                "failed to launch `{runtime}` — is it installed and on your PATH? \
             (override with DOCGEN_CONTAINER_RUNTIME)"
            )
        })?;

    if !status.success() {
        // A Ctrl-C (SIGINT) terminates the container normally from the user's
        // perspective; only surface a genuine non-zero/non-signal exit.
        anyhow::bail!("{runtime} exited with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_args_publish_port_and_image() {
        let args = container_args(9090, PLANTUML_IMAGE);
        assert_eq!(args, vec!["run", "--rm", "-p", "9090:8080", PLANTUML_IMAGE]);
    }

    #[test]
    fn runtime_defaults_to_docker_without_env() {
        // Not asserting on the env-set case (would mutate process-global env);
        // the default branch is the important contract.
        if std::env::var_os("DOCGEN_CONTAINER_RUNTIME").is_none() {
            assert_eq!(runtime(), "docker");
        }
    }
}
