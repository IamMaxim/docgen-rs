//! The networked [`PlantumlRenderer`] implementation: encode → cache lookup →
//! HTTP GET the SVG → cache store. Classifies failures into [`PlantumlError`] so
//! docgen-core can render a specific, detailed error component.

use std::path::{Path, PathBuf};
use std::time::Duration;

use docgen_core::{PlantumlError, PlantumlRenderer};
use sha2::{Digest, Sha256};

use crate::encode;

/// Connect+read timeout for a single diagram render. A hung server therefore
/// degrades to an `Unreachable` error component instead of stalling the build.
const RENDER_TIMEOUT: Duration = Duration::from_secs(10);

/// Renders PlantUML diagrams against an external server, caching each result on
/// disk (keyed by server URL + source) so unchanged diagrams never re-hit the
/// network — a full rebuild with the server down still succeeds for cached ones.
pub struct HttpRenderer {
    /// Server base URL, no trailing slash (SVG endpoint is `{server}/svg/{enc}`).
    server: String,
    /// The project's `.docgen/` scratch dir (self-ignored on first cache write).
    docgen_dir: PathBuf,
    /// Directory holding cached `<hash>.svg` files (`{docgen_dir}/plantuml-cache`).
    cache_dir: PathBuf,
    agent: ureq::Agent,
}

impl HttpRenderer {
    /// Build a renderer for `server`, caching under `{docgen_dir}/plantuml-cache`.
    /// `docgen_dir` is the project's `.docgen/` scratch dir. The cache directory
    /// is created lazily on the first successful render (with a `*` `.gitignore`
    /// so it is never committed), so a project that renders no diagrams never
    /// gets a stray `.docgen/`.
    pub fn new(server: impl Into<String>, docgen_dir: impl Into<PathBuf>) -> Self {
        let server = server.into().trim_end_matches('/').to_string();
        let docgen_dir = docgen_dir.into();
        let cache_dir = docgen_dir.join("plantuml-cache");
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(RENDER_TIMEOUT)
            .timeout_read(RENDER_TIMEOUT)
            .build();
        Self {
            server,
            docgen_dir,
            cache_dir,
            agent,
        }
    }

    /// Best-effort: ensure the cache dir exists and `.docgen/` self-ignores. Run
    /// lazily before the first cache write so a diagram-free build stays clean.
    fn ensure_cache_setup(&self) {
        let _ = std::fs::create_dir_all(&self.cache_dir);
        ensure_gitignore(&self.docgen_dir);
    }

    /// Cache-file path for a given diagram source (server URL folded into the key
    /// so switching servers never serves a stale render).
    fn cache_path(&self, source: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(self.server.as_bytes());
        h.update(b"\n");
        h.update(source.as_bytes());
        let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        self.cache_dir.join(format!("{hex}.svg"))
    }
}

impl PlantumlRenderer for HttpRenderer {
    fn render(&self, source: &str) -> Result<String, PlantumlError> {
        let cache_path = self.cache_path(source);
        if let Ok(cached) = std::fs::read_to_string(&cache_path) {
            return Ok(cached);
        }

        let url = format!("{}/svg/{}", self.server, encode::encode(source));
        match self.agent.get(&url).call() {
            Ok(resp) => {
                let svg = resp.into_string().map_err(|e| PlantumlError::Unreachable {
                    server: self.server.clone(),
                    detail: format!("reading response body: {e}"),
                })?;
                // Best-effort cache write (creating the cache dir lazily); the
                // render result is used regardless.
                self.ensure_cache_setup();
                let _ = std::fs::write(&cache_path, &svg);
                Ok(svg)
            }
            // Non-2xx: a diagram syntax error carries PlantUML's message/line in
            // response headers. Read the headers (owned) before optionally
            // consuming the body for a fallback snippet.
            Err(ureq::Error::Status(status, resp)) => {
                let header_msg = resp.header("X-PlantUML-Diagram-Error").map(str::to_string);
                let line = resp
                    .header("X-PlantUML-Diagram-Error-Line")
                    .and_then(|s| s.trim().parse::<u32>().ok());
                let message = match header_msg {
                    Some(m) => m,
                    None => {
                        // No structured header → a trimmed snippet of the body.
                        let body = resp.into_string().unwrap_or_default();
                        let snippet: String = body.trim().chars().take(200).collect();
                        if snippet.is_empty() {
                            "server returned an error with no detail".to_string()
                        } else {
                            snippet
                        }
                    }
                };
                Err(PlantumlError::Server {
                    status,
                    message,
                    line,
                })
            }
            Err(ureq::Error::Transport(t)) => Err(PlantumlError::Unreachable {
                server: self.server.clone(),
                detail: t.to_string(),
            }),
        }
    }
}

/// Write a `*` `.gitignore` into `dir` so the cache is never committed. No-op if
/// the file already exists; failures are ignored (cache is scratch either way).
fn ensure_gitignore(dir: &Path) {
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(&gi, "*\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_does_not_create_cache_dir_eagerly() {
        // A diagram-free project must not get a stray `.docgen/`: construction is
        // side-effect-free; the dir appears only on the first cache write.
        let tmp = tempfile::tempdir().unwrap();
        let docgen_dir = tmp.path().join(".docgen");
        let _r = HttpRenderer::new("http://localhost:8080/", &docgen_dir);
        assert!(!docgen_dir.exists(), "no .docgen until a diagram renders");
    }

    #[test]
    fn ensure_cache_setup_creates_dir_and_self_ignores() {
        let tmp = tempfile::tempdir().unwrap();
        let docgen_dir = tmp.path().join(".docgen");
        let r = HttpRenderer::new("http://localhost:8080/", &docgen_dir);
        r.ensure_cache_setup();
        assert!(docgen_dir.join("plantuml-cache").is_dir());
        let gi = std::fs::read_to_string(docgen_dir.join(".gitignore")).unwrap();
        assert_eq!(gi.trim(), "*");
    }

    #[test]
    fn cache_path_depends_on_server_and_source() {
        let tmp = tempfile::tempdir().unwrap();
        let a = HttpRenderer::new("http://a", tmp.path().join(".docgen"));
        let b = HttpRenderer::new("http://b", tmp.path().join(".docgen"));
        // Same source, different server → different cache keys.
        assert_ne!(a.cache_path("@startuml\n@enduml"), b.cache_path("@startuml\n@enduml"));
        // Same server, different source → different keys.
        assert_ne!(a.cache_path("one"), a.cache_path("two"));
        // Stable for identical inputs.
        assert_eq!(a.cache_path("x"), a.cache_path("x"));
    }

    #[test]
    fn trailing_slash_on_server_is_trimmed() {
        let tmp = tempfile::tempdir().unwrap();
        let r = HttpRenderer::new("http://localhost:8080/", tmp.path().join(".docgen"));
        assert_eq!(r.server, "http://localhost:8080");
    }

    #[test]
    fn a_cached_svg_is_returned_without_network() {
        // Pre-seed the cache; render() must return it without touching the network
        // (the server URL is unroutable, so a network attempt would error).
        let tmp = tempfile::tempdir().unwrap();
        let r = HttpRenderer::new("http://127.0.0.1:1", tmp.path().join(".docgen"));
        let src = "@startuml\nA->B\n@enduml";
        r.ensure_cache_setup();
        std::fs::write(r.cache_path(src), "<svg>CACHED</svg>").unwrap();
        let out = r.render(src).expect("cache hit");
        assert_eq!(out, "<svg>CACHED</svg>");
    }
}
