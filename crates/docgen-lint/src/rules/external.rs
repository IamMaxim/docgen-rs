//! Opt-in EXTERNAL rules: checks that leave the process (PlantUML server,
//! the `mmdc` CLI, live HTTP requests). Each rule is gated by its own config
//! toggle and does nothing when the toggle is off — the rule stays registered
//! so `--list-rules` shows it, but `check()` returns immediately.
//!
//! When the external dependency itself is unavailable (server unreachable,
//! `mmdc` not installed), each rule emits ONE explicitly-Info "check skipped"
//! diagnostic and stops, instead of spamming a failure per diagram/URL. The
//! engine preserves such explicitly non-default severities.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use docgen_core::PlantumlError;
use docgen_core::PlantumlRenderer as _;
use globset::{Glob, GlobMatcher};

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::diagrams::walk_directives;
use crate::rules::util::line32;
use crate::rules::Rule;

/// Validate every PlantUML source against the PlantUML server (the same
/// renderer + on-disk cache `docgen build` uses).
pub struct PlantumlSyntax;

impl Rule for PlantumlSyntax {
    fn id(&self) -> &'static str {
        "plantuml-syntax"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "plantuml diagram fails server-side syntax validation (off unless [lint.plantuml] check-syntax = true)"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        if !ctx.config.lint.plantuml.check_syntax {
            return;
        }

        // Collect every diagram source to validate: (file, directive line,
        // source). Inline `:::plantuml` bodies are re-extracted the same way
        // `plantuml-empty` does, so reported lines match the source file.
        let mut jobs: Vec<(String, Option<u32>, String)> = Vec::new();
        for doc in &ctx.docs {
            walk_directives(&doc.prepared.body_md, 0, &mut |inst, line| {
                if inst.name != "plantuml" {
                    return;
                }
                let has_src = inst.attrs.get("src").is_some_and(|s| !s.trim().is_empty());
                if has_src || inst.inner_md.trim().is_empty() {
                    return; // `src=` forms come from `ctx.diagrams`; empty is `plantuml-empty`'s
                }
                jobs.push((
                    doc.prepared.rel_path.clone(),
                    line32(line),
                    inst.inner_md.clone(),
                ));
            });
        }
        // `ctx.diagrams` is a path → source map, so a `.puml` referenced by
        // several directives is still validated exactly once, attributed to
        // the `.puml` file itself.
        for (rel_path, source) in &ctx.diagrams {
            jobs.push((rel_path.clone(), None, source.clone()));
        }
        if jobs.is_empty() {
            return;
        }

        // Mirror `docgen build`'s renderer wiring: server resolved from
        // config/env, cache under `{project_root}/.docgen` — so lint shares
        // the build's disk cache and cached diagrams never re-hit the network.
        let server = docgen_config::resolve_plantuml_server(&ctx.config.plantuml.server);
        let renderer = docgen_plantuml::HttpRenderer::new(server, ctx.project_root.join(".docgen"));

        for (file, directive_line, source) in jobs {
            match renderer.render(&source) {
                Ok(_) => {}
                Err(PlantumlError::Server {
                    status,
                    message,
                    line,
                }) => {
                    // Best source position: directive line + diagram-relative
                    // line when both are known; else the directive line; for a
                    // `.puml` file the server's line IS the file line.
                    let at = match (directive_line, line) {
                        (Some(d), Some(l)) => Some(d.saturating_add(l)),
                        (Some(d), None) => Some(d),
                        (None, l) => l,
                    };
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file,
                        line: at,
                        col: None,
                        message: format!("plantuml syntax error (HTTP {status}): {message}"),
                        note: Some("reported by PlantUML server".to_string()),
                    });
                }
                Err(PlantumlError::Unreachable { server, detail }) => {
                    // One skip notice, not one failure per diagram. Explicit
                    // Info: the engine keeps non-default severities as-is.
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: Severity::Info,
                        file,
                        line: None,
                        col: None,
                        message: format!(
                            "PlantUML server {server} is unreachable; plantuml-syntax skipped"
                        ),
                        note: Some(detail),
                    });
                    return;
                }
            }
        }
    }
}

/// How long one `mmdc` invocation may run before it is killed. Guards against
/// a hung headless browser stalling the whole lint run.
const MMDC_TIMEOUT: Duration = Duration::from_secs(30);

/// Wait for `child` with a manual poll loop (std has no `wait_timeout`).
/// `Ok(None)` means the timeout elapsed and the child was killed + reaped.
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The last ~3 non-empty stderr lines, trimmed and joined — enough of mmdc's
/// parse error to be useful in a one-line note.
fn stderr_tail(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(lines[lines.len().saturating_sub(3)..].join(" | "))
}

/// Validate every mermaid fence by running the configured `mmdc` CLI on it.
///
/// mermaid-cli has no parse-only flag (its options are input/output/theme/…),
/// so validation renders each fence to a throwaway SVG in a temp dir:
/// `mmdc --input <fence.mmd> --output <out.svg>` — nonzero exit is a syntax
/// error, with the stderr tail carried in the note.
pub struct MermaidSyntax;

impl Rule for MermaidSyntax {
    fn id(&self) -> &'static str {
        "mermaid-syntax"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "mermaid fence fails mmdc validation (off unless [lint.mermaid] check-syntax = true)"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        if !ctx.config.lint.mermaid.check_syntax {
            return;
        }
        let mmdc = &ctx.config.lint.mermaid.mmdc;
        let Ok(dir) = tempfile::tempdir() else {
            return; // no temp dir, nothing sane to do
        };

        let mut n = 0usize;
        for doc in &ctx.docs {
            for fence in doc.refs.fences.iter().filter(|f| f.lang == "mermaid") {
                if fence.body.trim().is_empty() {
                    continue; // `mermaid-empty`'s finding, and mmdc can't parse it anyway
                }
                n += 1;
                let input = dir.path().join(format!("fence-{n}.mmd"));
                let output = dir.path().join(format!("fence-{n}.svg"));
                if std::fs::write(&input, &fence.body).is_err() {
                    continue;
                }

                let spawned = Command::new(mmdc)
                    .arg("--input")
                    .arg(&input)
                    .arg("--output")
                    .arg(&output)
                    .arg("--quiet")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn();
                let mut child = match spawned {
                    Ok(c) => c,
                    Err(e) => {
                        // Binary missing/unrunnable: one skip notice, then stop.
                        out.push(Diagnostic {
                            rule: self.id(),
                            severity: Severity::Info,
                            file: doc.prepared.rel_path.clone(),
                            line: None,
                            col: None,
                            message: format!(
                                "mmdc not found (configured: `{mmdc}`); mermaid-syntax skipped"
                            ),
                            note: Some(e.to_string()),
                        });
                        return;
                    }
                };

                // Drain stderr on a thread so a chatty child can never
                // deadlock against a full pipe while we poll for exit.
                let stderr_pipe = child.stderr.take();
                let reader = std::thread::spawn(move || {
                    let mut buf = String::new();
                    if let Some(mut pipe) = stderr_pipe {
                        let _ = pipe.read_to_string(&mut buf);
                    }
                    buf
                });
                let status = wait_with_timeout(&mut child, MMDC_TIMEOUT);
                let stderr = reader.join().unwrap_or_default();

                match status {
                    Ok(Some(s)) if s.success() => {}
                    Ok(Some(s)) => out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(fence.line),
                        col: None,
                        message: format!(
                            "mermaid diagram failed mmdc validation (exit {})",
                            s.code().map_or_else(|| "signal".into(), |c| c.to_string())
                        ),
                        note: stderr_tail(&stderr),
                    }),
                    // Timed out (killed) — report it; a hung mmdc is actionable.
                    Ok(None) => out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(fence.line),
                        col: None,
                        message: format!(
                            "mmdc timed out after {}s validating this fence (killed)",
                            MMDC_TIMEOUT.as_secs()
                        ),
                        note: stderr_tail(&stderr),
                    }),
                    Err(_) => {} // try_wait failed; nothing trustworthy to report
                }
            }
        }
    }
}

/// First occurrence + site-wide count for one deduped external URL.
struct UrlOccurrence {
    file: String,
    line: Option<u32>,
    count: usize,
}

/// True when `url` is an external URL this rule should probe: http(s), and
/// not a loopback host (`localhost` / `127.0.0.1`, with or without a port).
fn checkable_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    !(host == "localhost" || host == "127.0.0.1")
}

/// Probe one URL: HEAD first; on 405/404-from-HEAD or any HEAD transport
/// error, retry once with GET (some servers reject HEAD outright). `Some` is
/// the failure text for the diagnostic; `None` means the URL is fine.
fn check_url(agent: &ureq::Agent, url: &str) -> Option<String> {
    match agent.head(url).call() {
        Ok(_) => None,
        Err(ureq::Error::Status(status, _)) if status != 404 && status != 405 => {
            Some(format!("returned HTTP {status}"))
        }
        Err(_) => match agent.get(url).call() {
            Ok(_) => None,
            Err(ureq::Error::Status(status, _)) => Some(format!("returned HTTP {status}")),
            Err(ureq::Error::Transport(t)) => Some(format!("unreachable: {t}")),
        },
    }
}

/// Check that external `http(s)://` links and images actually respond.
///
/// Default severity is `Allow`, so the engine skips this rule entirely unless
/// `[lint.rules] external-url = "warn"` (or `"error"`/`"info"`) opts in.
pub struct ExternalUrl;

impl Rule for ExternalUrl {
    fn id(&self) -> &'static str {
        "external-url"
    }
    fn default_severity(&self) -> Severity {
        Severity::Allow
    }
    fn description(&self) -> &'static str {
        "external URL is unreachable or returns an HTTP error (off unless [lint.rules] external-url raises the severity)"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        // Compile `[lint.external-urls] exclude` globs (matched against the
        // full URL string). A bad glob is one Info notice, then we proceed
        // WITHOUT excludes rather than aborting the whole rule.
        let mut excludes: Vec<GlobMatcher> = Vec::new();
        for pattern in &ctx.config.lint.external_urls.exclude {
            match Glob::new(pattern) {
                Ok(g) => excludes.push(g.compile_matcher()),
                Err(e) => {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: Severity::Info,
                        file: "docgen.toml".to_string(),
                        line: None,
                        col: None,
                        message: format!(
                            "invalid [lint.external-urls] exclude glob `{pattern}`; checking without excludes"
                        ),
                        note: Some(e.to_string()),
                    });
                    excludes.clear();
                    break;
                }
            }
        }

        // Dedupe URLs site-wide; the first occurrence (doc discovery order,
        // then source order) wins attribution.
        let mut urls: BTreeMap<String, UrlOccurrence> = BTreeMap::new();
        for doc in &ctx.docs {
            for link in &doc.refs.links {
                if !checkable_url(&link.url) || excludes.iter().any(|m| m.is_match(&link.url)) {
                    continue;
                }
                urls.entry(link.url.clone())
                    .and_modify(|o| o.count += 1)
                    .or_insert_with(|| UrlOccurrence {
                        file: doc.prepared.rel_path.clone(),
                        line: line32(link.line),
                        count: 1,
                    });
            }
        }
        if urls.is_empty() {
            return;
        }

        // Probe with bounded concurrency: min(8, urls) std threads pulling
        // from a shared atomic index over a snapshot of the URL list.
        let timeout = Duration::from_secs(ctx.config.lint.external_urls.timeout_secs);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .build();
        let todo: Vec<&str> = urls.keys().map(String::as_str).collect();
        let next = AtomicUsize::new(0);
        let failures: Mutex<BTreeMap<&str, String>> = Mutex::new(BTreeMap::new());
        std::thread::scope(|scope| {
            for _ in 0..todo.len().min(8) {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(url) = todo.get(i) else { break };
                    if let Some(failure) = check_url(&agent, url) {
                        failures.lock().unwrap().insert(url, failure);
                    }
                });
            }
        });
        let failures = failures.into_inner().unwrap();

        // Emit in sorted-URL order (deterministic; the engine re-sorts by
        // file/line anyway) at the default severity, so the engine re-levels
        // to whatever the config opted in with.
        for (url, occurrence) in &urls {
            let Some(failure) = failures.get(url.as_str()) else {
                continue;
            };
            out.push(Diagnostic {
                rule: self.id(),
                severity: self.default_severity(),
                file: occurrence.file.clone(),
                line: occurrence.line,
                col: None,
                message: format!("external URL {url} {failure}"),
                note: (occurrence.count > 1)
                    .then(|| format!("appears {} times site-wide", occurrence.count)),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{run_with_rules, LintOptions};
    use crate::rules::all_rules;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::path::Path;

    /// Write `files` (docs-relative; `docgen.toml` at the root) into a temp
    /// project. Like `test_fixture::lint_fixture` but the tempdir is returned
    /// so tests can add executables / derive addresses first.
    fn write_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
        for (path, contents) in files {
            let full = if *path == "docgen.toml" {
                tmp.path().join(path)
            } else {
                tmp.path().join("docs").join(path)
            };
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        tmp
    }

    /// Lint `root` with only `rule` selected, through the real engine.
    fn lint(root: &Path, rule: &str) -> Vec<Diagnostic> {
        let options = LintOptions {
            only_rules: Some(vec![rule.to_string()]),
            deny_warnings: false,
        };
        run_with_rules(root, &options, &all_rules())
            .unwrap()
            .diagnostics
    }

    /// Minimal HTTP/1.1 stub: accepts connections on a background thread and
    /// answers each request via `respond(method, path)` (a full raw response).
    /// Returns the base URL. `bind` picks the loopback family — external-url
    /// tests use `[::1]` because the rule deliberately skips `127.0.0.1`.
    fn stub_http(bind: &str, respond: impl Fn(&str, &str) -> String + Send + 'static) -> String {
        let listener = TcpListener::bind(bind).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match std::io::Read::read(&mut stream, &mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => break,
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let mut parts = head.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                let _ = stream.write_all(respond(&method, &path).as_bytes());
            }
        });
        format!("http://{addr}")
    }

    /// A raw HTTP/1.1 response. HEAD replies get no body (Content-Length 0).
    fn response(method: &str, status: u16, reason: &str, headers: &str, body: &str) -> String {
        let body = if method == "HEAD" { "" } else { body };
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    // ---- plantuml-syntax ---------------------------------------------------

    fn plantuml_toml(server: &str) -> String {
        format!("[plantuml]\nserver = \"{server}\"\n\n[lint.plantuml]\ncheck-syntax = true\n")
    }

    #[test]
    fn plantuml_syntax_passes_valid_diagrams() {
        let server = stub_http("127.0.0.1:0", |method, _path| {
            response(method, 200, "OK", "", "<svg>ok</svg>")
        });
        let tmp = write_project(&[
            ("docgen.toml", &plantuml_toml(&server)),
            (
                "index.md",
                "# T\n\n:::plantuml\n@startuml\nA -> B\n@enduml\n:::\n",
            ),
            ("d.puml", "@startuml\nB -> C\n@enduml\n"),
        ]);
        let diags = lint(tmp.path(), "plantuml-syntax");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn plantuml_syntax_reports_server_error_with_combined_line() {
        let server = stub_http("127.0.0.1:0", |method, _path| {
            response(
                method,
                400,
                "Bad Request",
                "X-PlantUML-Diagram-Error: Syntax Error?\r\nX-PlantUML-Diagram-Error-Line: 2\r\n",
                "bad",
            )
        });
        // The directive opens on line 3; the server blames diagram line 2.
        let tmp = write_project(&[
            ("docgen.toml", &plantuml_toml(&server)),
            (
                "index.md",
                "# T\n\n:::plantuml\n@startuml\nbroken here\n@enduml\n:::\n",
            ),
        ]);
        let diags = lint(tmp.path(), "plantuml-syntax");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].file, "index.md");
        assert_eq!(diags[0].line, Some(3 + 2));
        assert!(diags[0].message.contains("Syntax Error?"), "{diags:?}");
        assert_eq!(
            diags[0].note.as_deref(),
            Some("reported by PlantUML server")
        );
    }

    #[test]
    fn plantuml_syntax_checks_puml_files_at_the_server_line() {
        let server = stub_http("127.0.0.1:0", |method, _path| {
            response(
                method,
                400,
                "Bad Request",
                "X-PlantUML-Diagram-Error: Syntax Error?\r\nX-PlantUML-Diagram-Error-Line: 2\r\n",
                "bad",
            )
        });
        let tmp = write_project(&[
            ("docgen.toml", &plantuml_toml(&server)),
            (
                "index.md",
                ":::plantuml{src=d.puml}\n:::\n\n:::plantuml{src=d.puml}\n:::\n",
            ),
            ("d.puml", "@startuml\nbroken\n@enduml\n"),
        ]);
        let diags = lint(tmp.path(), "plantuml-syntax");
        // Referenced twice, validated once — attributed to the .puml file.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "d.puml");
        assert_eq!(diags[0].line, Some(2));
    }

    #[test]
    fn plantuml_syntax_unreachable_server_is_one_info_skip() {
        // Bind then drop a listener so the port is closed (connection refused).
        let addr = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let tmp = write_project(&[
            ("docgen.toml", &plantuml_toml(&format!("http://{addr}"))),
            (
                "index.md",
                ":::plantuml\n@startuml\na\n@enduml\n:::\n\n:::plantuml\n@startuml\nb\n@enduml\n:::\n",
            ),
            ("d.puml", "@startuml\nc\n@enduml\n"),
        ]);
        let diags = lint(tmp.path(), "plantuml-syntax");
        // One Info notice for three diagrams — and it stays Info despite the
        // rule's Error default (the engine keeps explicit non-default levels).
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(diags[0].message.contains("unreachable"), "{diags:?}");
        assert!(diags[0].message.contains("skipped"), "{diags:?}");
    }

    #[test]
    fn plantuml_syntax_is_off_without_the_toggle() {
        // Broken diagram + no server, but check-syntax is absent → silence.
        let tmp = write_project(&[
            (
                "docgen.toml",
                "[plantuml]\nserver = \"http://127.0.0.1:1\"\n",
            ),
            ("index.md", ":::plantuml\n@startuml\nbroken\n@enduml\n:::\n"),
        ]);
        let diags = lint(tmp.path(), "plantuml-syntax");
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ---- mermaid-syntax ----------------------------------------------------

    /// Write an executable fake `mmdc` shell script and return its path.
    #[cfg(unix)]
    fn fake_mmdc(dir: &Path, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("mmdc");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn mermaid_toml(mmdc: &str) -> String {
        format!("[lint.mermaid]\ncheck-syntax = true\nmmdc = \"{mmdc}\"\n")
    }

    #[cfg(unix)]
    #[test]
    fn mermaid_syntax_passes_when_mmdc_exits_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let mmdc = fake_mmdc(tmp.path(), "#!/bin/sh\nexit 0\n");
        let project = write_project(&[
            ("docgen.toml", &mermaid_toml(&mmdc)),
            ("index.md", "# T\n\n```mermaid\ngraph TD\nA-->B\n```\n"),
        ]);
        let diags = lint(project.path(), "mermaid-syntax");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[cfg(unix)]
    #[test]
    fn mermaid_syntax_reports_failure_with_stderr_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let mmdc = fake_mmdc(
            tmp.path(),
            "#!/bin/sh\necho 'Parse error on line 2:' >&2\necho \"Expecting 'SPACE'\" >&2\nexit 1\n",
        );
        let project = write_project(&[
            ("docgen.toml", &mermaid_toml(&mmdc)),
            ("index.md", "# T\n\n```mermaid\ngraph TD\nA--?>B\n```\n"),
        ]);
        let diags = lint(project.path(), "mermaid-syntax");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(diags[0].line, Some(3)); // the fence line
        assert!(diags[0].message.contains("exit 1"), "{diags:?}");
        let note = diags[0].note.as_deref().unwrap();
        assert!(note.contains("Parse error on line 2:"), "{note}");
        assert!(note.contains("Expecting 'SPACE'"), "{note}");
    }

    #[test]
    fn mermaid_syntax_missing_binary_is_one_info_skip() {
        let project = write_project(&[
            ("docgen.toml", &mermaid_toml("/nonexistent/docgen-mmdc")),
            (
                "index.md",
                "```mermaid\ngraph TD\n```\n\n```mermaid\npie\n```\n",
            ),
        ]);
        let diags = lint(project.path(), "mermaid-syntax");
        // One Info for the whole run, not one per fence.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Info);
        assert!(diags[0].message.contains("mmdc not found"), "{diags:?}");
        assert!(diags[0].message.contains("skipped"), "{diags:?}");
    }

    #[cfg(unix)]
    #[test]
    fn mermaid_syntax_is_off_without_the_toggle() {
        let tmp = tempfile::tempdir().unwrap();
        let mmdc = fake_mmdc(tmp.path(), "#!/bin/sh\nexit 1\n");
        let project = write_project(&[
            (
                "docgen.toml",
                &format!("[lint.mermaid]\nmmdc = \"{mmdc}\"\n"),
            ),
            ("index.md", "```mermaid\ngraph TD\n```\n"),
        ]);
        let diags = lint(project.path(), "mermaid-syntax");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn stderr_tail_keeps_last_three_nonempty_lines() {
        assert_eq!(stderr_tail(""), None);
        assert_eq!(stderr_tail("\n  \n"), None);
        assert_eq!(stderr_tail("one\n"), Some("one".to_string()));
        assert_eq!(
            stderr_tail("a\nb\n\nc\n  d  \n"),
            Some("b | c | d".to_string())
        );
    }

    // ---- external-url ------------------------------------------------------

    /// Stub for the URL tests, bound to `[::1]` (the rule skips `127.0.0.1`):
    /// `/ok` → 200; `/missing` → 404; `/headless` → 405 for HEAD, 200 for GET.
    fn url_stub() -> String {
        stub_http("[::1]:0", |method, path| match (method, path) {
            ("HEAD", "/headless") => response(method, 405, "Method Not Allowed", "", ""),
            (_, "/headless") | (_, "/ok") => response(method, 200, "OK", "", "hello"),
            _ => response(method, 404, "Not Found", "", "gone"),
        })
    }

    #[test]
    fn external_url_flags_only_broken_urls_and_dedupes() {
        let server = url_stub();
        let tmp = write_project(&[
            (
                "docgen.toml",
                "[lint.rules]\n\"external-url\" = \"warn\"\n\n[lint.external-urls]\ntimeout-secs = 5\nexclude = [\"*skipme*\"]\n",
            ),
            (
                "index.md",
                &format!(
                    "# T\n\n[ok]({server}/ok)\n[gone]({server}/missing)\n\
                     [head]({server}/headless)\n[ex]({server}/skipme/x)\n\
                     [local](http://localhost:1/x)\n[loop](http://127.0.0.1:1/x)\n\
                     ![img]({server}/ok)\n"
                ),
            ),
            ("z.md", &format!("[gone again]({server}/missing)\n")),
        ]);
        let diags = lint(tmp.path(), "external-url");
        // Only /missing fails: /ok + /headless pass, the exclude glob and the
        // loopback hosts are skipped, and the duplicate is deduped site-wide.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Warn); // re-leveled from Allow
        assert_eq!(diags[0].file, "index.md"); // first occurrence wins
        assert_eq!(diags[0].line, Some(4));
        assert!(diags[0].message.contains("returned HTTP 404"), "{diags:?}");
        assert_eq!(diags[0].note.as_deref(), Some("appears 2 times site-wide"));
    }

    #[test]
    fn external_url_reports_unreachable_hosts() {
        let addr = {
            let l = TcpListener::bind("[::1]:0").unwrap();
            l.local_addr().unwrap()
        };
        let tmp = write_project(&[
            (
                "docgen.toml",
                "[lint.rules]\n\"external-url\" = \"error\"\n\n[lint.external-urls]\ntimeout-secs = 2\n",
            ),
            ("index.md", &format!("[dead](http://{addr}/x)\n")),
        ]);
        let diags = lint(tmp.path(), "external-url");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("unreachable:"), "{diags:?}");
    }

    #[test]
    fn external_url_is_off_by_default() {
        // A 404 link, but no [lint.rules] opt-in → the Allow default means the
        // engine never runs the rule (and no network is touched).
        let server = url_stub();
        let tmp = write_project(&[("index.md", &format!("[gone]({server}/missing)\n"))]);
        let diags = lint(tmp.path(), "external-url");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn external_url_bad_exclude_glob_is_info_and_checks_proceed() {
        let server = url_stub();
        let tmp = write_project(&[
            (
                "docgen.toml",
                "[lint.rules]\n\"external-url\" = \"warn\"\n\n[lint.external-urls]\nexclude = [\"bad[\"]\n",
            ),
            (
                "index.md",
                &format!("[ok]({server}/ok)\n[gone]({server}/missing)\n"),
            ),
        ]);
        let diags = lint(tmp.path(), "external-url");
        assert_eq!(diags.len(), 2, "{diags:?}");
        let info = diags.iter().find(|d| d.severity == Severity::Info).unwrap();
        assert!(info.message.contains("bad["), "{info:?}");
        assert_eq!(info.file, "docgen.toml");
        // The URL check still ran without excludes.
        let warn = diags.iter().find(|d| d.severity == Severity::Warn).unwrap();
        assert!(warn.message.contains("returned HTTP 404"), "{warn:?}");
    }

    #[test]
    fn checkable_url_filters_schemes_and_loopback() {
        assert!(checkable_url("http://example.com/a"));
        assert!(checkable_url("https://example.com"));
        assert!(checkable_url("http://[::1]:8080/x")); // only named loopbacks skip
        assert!(!checkable_url("http://localhost/x"));
        assert!(!checkable_url("http://localhost:3000/x"));
        assert!(!checkable_url("http://127.0.0.1/x"));
        assert!(!checkable_url("http://127.0.0.1:8080/x"));
        assert!(!checkable_url("mailto:a@b.c"));
        assert!(!checkable_url("ftp://example.com/x"));
        assert!(!checkable_url("/relative/path"));
    }
}
