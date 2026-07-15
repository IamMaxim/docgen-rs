//! Build-time PlantUML directive support.
//!
//! docgen-core stays network-free: this module defines the [`PlantumlRenderer`]
//! trait (implemented by the `docgen-plantuml` crate, which owns the HTTP client,
//! encoding and cache) and the pure directive glue that turns a `:::plantuml`
//! instance into HTML. The renderer is injected into the render pipeline exactly
//! like [`crate::asseturl::AssetUrlResolver`] — so nothing here depends on the
//! network implementation, and the whole thing is unit-testable with a mock.
//!
//! A `:::plantuml` directive references its diagram source one of two ways:
//! a `src` attribute (a `.puml` file, resolved relative/absolute against the
//! doc's directory) or the block body (inline PlantUML source). On success the
//! rendered SVG is embedded inline in the page; on ANY failure a detailed,
//! visible error component is produced and the build still succeeds.

use crate::directivepass::DirectiveInstance;
use crate::pipeline::Diagrams;
use crate::util::escape_html;

/// The PlantUML rendering context threaded through the render pipeline: the
/// preloaded `.puml` source map plus an optional renderer. Bundled so the many
/// pipeline functions carry a single `Option<&PlantumlSupport>` param — `None`
/// (feature off / not wired) makes every `:::plantuml` emit a "disabled" notice.
pub struct PlantumlSupport<'a> {
    /// Docs-relative path → raw PlantUML source, for `src=` file references.
    pub diagrams: &'a Diagrams,
    /// The concrete renderer (network + cache); `None` renders a disabled notice.
    pub renderer: Option<&'a dyn PlantumlRenderer>,
}

/// Renders PlantUML source text to an SVG document. Implemented by
/// `docgen-plantuml` (network + on-disk cache); injected into the render
/// pipeline as `Option<&dyn PlantumlRenderer>`. Kept in core so the directive
/// glue never depends on the concrete networked implementation.
pub trait PlantumlRenderer {
    /// Render `source` (raw PlantUML text) to an SVG document string, or return a
    /// classified [`PlantumlError`] carrying enough detail for a specific error
    /// component.
    fn render(&self, source: &str) -> Result<String, PlantumlError>;
}

/// A classified PlantUML render failure. Carries specifics so the error
/// component is never a generic "an error occurred".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlantumlError {
    /// The server could not be reached (connection refused, DNS, TLS, timeout).
    Unreachable {
        /// The server URL that was tried.
        server: String,
        /// The underlying transport error text.
        detail: String,
    },
    /// The server returned a non-success status. For a diagram syntax error the
    /// PlantUML server responds `400` with `X-PlantUML-Diagram-Error` /
    /// `-Error-Line` headers, surfaced here as `message`/`line`.
    Server {
        /// HTTP status code.
        status: u16,
        /// PlantUML error message (from the response header/body).
        message: String,
        /// 1-based source line of the error, when the server reported one.
        line: Option<u32>,
    },
}

/// Render one `:::plantuml` directive instance to HTML.
///
/// Resolves the diagram source (from the `src` file map or the inline body),
/// then either calls `renderer` (feature on) or emits an inert "disabled" notice
/// (`renderer` is `None`). Every failure path yields a `.docgen-plantuml-error`
/// block rather than panicking, so a bad diagram never fails the build.
///
/// * `base_dir` — the referencing doc's docs-relative directory, for resolving a
///   relative `src`.
/// * `support` — the diagram map + renderer; `None` means the feature is off.
/// * `id` — a stable per-directive id (used as an HTML anchor for the container).
pub fn render_directive(
    inst: &DirectiveInstance,
    base_dir: &str,
    support: Option<&PlantumlSupport>,
    id: &str,
) -> String {
    // An empty map to resolve against when the feature is off (so a bad `src`
    // still reports "not found" rather than panicking).
    let empty = Diagrams::new();
    let diagrams = support.map(|s| s.diagrams).unwrap_or(&empty);
    let renderer = support.and_then(|s| s.renderer);

    // 1. Resolve the diagram source + a human-facing identity for error messages.
    let src_attr = inst.attrs.get("src").map(|s| s.trim()).unwrap_or("");
    let (identity, source): (String, String) = if !src_attr.is_empty() {
        let Some(key) = crate::pipeline::resolve_include_key(base_dir, src_attr) else {
            return error_html(
                id,
                src_attr,
                &format!("diagram source path `{src_attr}` escapes the docs root"),
            );
        };
        match diagrams.get(&key) {
            Some(s) => (src_attr.to_string(), s.clone()),
            None => {
                return error_html(
                    id,
                    src_attr,
                    &format!("diagram source file not found: `{key}`"),
                )
            }
        }
    } else {
        let body = inst.inner_md.trim();
        if body.is_empty() {
            return error_html(
                id,
                "plantuml",
                "`:::plantuml` needs a `src=\"file.puml\"` attribute or inline diagram source",
            );
        }
        ("inline diagram".to_string(), inst.inner_md.clone())
    };

    // 2. Render (or report the feature is off).
    let Some(renderer) = renderer else {
        return error_html(
            id,
            &identity,
            "PlantUML rendering is disabled (`[features] plantuml = false`, or no server wired)",
        );
    };
    match renderer.render(&source) {
        Ok(svg) => container_html(id, &identity, &svg),
        Err(e) => error_html(id, &identity, &describe_error(&e)),
    }
}

/// Human-readable one-line description of a [`PlantumlError`], with all the
/// specifics (server URL, status, PlantUML message + line).
fn describe_error(e: &PlantumlError) -> String {
    match e {
        PlantumlError::Unreachable { server, detail } => {
            format!("could not reach PlantUML server at {server}: {detail}")
        }
        PlantumlError::Server {
            status,
            message,
            line,
        } => match line {
            Some(l) => format!("server error (HTTP {status}) at line {l}: {message}"),
            None => format!("server error (HTTP {status}): {message}"),
        },
    }
}

/// Wrap rendered SVG in the scrollable container. The SVG's XML prolog/DOCTYPE is
/// stripped so only the `<svg>` element is inlined. SVG is author-trusted content
/// (same trust model as `render.unsafe = true` markdown).
fn container_html(id: &str, identity: &str, svg: &str) -> String {
    format!(
        "<div class=\"docgen-plantuml\" id=\"{id}\" data-plantuml=\"{}\">{}</div>",
        escape_html(identity),
        svg_body(svg)
    )
}

/// Strip everything before the first `<svg` (XML declaration, DOCTYPE, comments)
/// so the fragment embeds cleanly in HTML. If no `<svg` is found the input is
/// returned trimmed (the caller only reaches here on a 2xx SVG response).
fn svg_body(svg: &str) -> &str {
    match svg.find("<svg") {
        Some(i) => svg[i..].trim_end(),
        None => svg.trim(),
    }
}

/// A visible, styled error component. `identity` is the diagram (a `src` path or
/// "inline diagram"); `reason` is the specific failure. Both are HTML-escaped.
fn error_html(id: &str, identity: &str, reason: &str) -> String {
    let safe_id = escape_html(identity);
    let safe_reason = escape_html(reason);
    format!(
        "<div class=\"docgen-plantuml-error\" id=\"{id}\" data-plantuml=\"{safe_id}\">\
         <strong>PlantUML diagram failed</strong> \
         <span class=\"docgen-plantuml-error__id\">({safe_id})</span>\
         <span class=\"docgen-plantuml-error__reason\">{safe_reason}</span></div>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directivepass::extract;

    /// A mock renderer: records how many times it was called and returns a
    /// canned result so directive glue is testable without a network.
    struct Mock {
        result: Result<String, PlantumlError>,
        calls: std::cell::Cell<usize>,
    }
    impl Mock {
        fn ok(svg: &str) -> Self {
            Self {
                result: Ok(svg.to_string()),
                calls: std::cell::Cell::new(0),
            }
        }
        fn err(e: PlantumlError) -> Self {
            Self {
                result: Err(e),
                calls: std::cell::Cell::new(0),
            }
        }
    }
    impl PlantumlRenderer for Mock {
        fn render(&self, _source: &str) -> Result<String, PlantumlError> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    fn one_directive(md: &str) -> DirectiveInstance {
        let (_html, mut inst) = extract(md);
        assert_eq!(inst.len(), 1, "expected exactly one directive in {md:?}");
        inst.remove(0)
    }

    fn diagrams(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// Build a [`PlantumlSupport`] and render, so tests keep a compact shape.
    fn run(
        inst: &DirectiveInstance,
        base: &str,
        map: &Diagrams,
        renderer: Option<&dyn PlantumlRenderer>,
        id: &str,
    ) -> String {
        let support = PlantumlSupport {
            diagrams: map,
            renderer,
        };
        render_directive(inst, base, Some(&support), id)
    }

    #[test]
    fn inline_source_renders_svg_container() {
        let inst = one_directive(":::plantuml\n@startuml\nA->B\n@enduml\n:::\n");
        let mock = Mock::ok("<?xml version=\"1.0\"?>\n<svg>DIAGRAM</svg>\n");
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-0");
        assert_eq!(mock.calls.get(), 1);
        assert!(html.contains("docgen-plantuml\""));
        assert!(html.contains("<svg>DIAGRAM</svg>")); // prolog stripped
        assert!(!html.contains("<?xml")); // XML declaration removed
        assert!(html.contains("data-plantuml=\"inline diagram\""));
    }

    #[test]
    fn src_file_is_resolved_from_the_diagram_map() {
        let inst = one_directive(":::plantuml{src=\"../uml/a.puml\"}\n:::\n");
        let mock = Mock::ok("<svg>FROMFILE</svg>");
        let map = diagrams(&[("uml/a.puml", "@startuml\nA->B\n@enduml")]);
        let html = run(&inst, "guide", &map, Some(&mock), "d-1");
        assert_eq!(mock.calls.get(), 1);
        assert!(html.contains("<svg>FROMFILE</svg>"));
        assert!(html.contains("data-plantuml=\"../uml/a.puml\""));
    }

    #[test]
    fn src_wins_over_inline_body() {
        let inst = one_directive(":::plantuml{src=\"a.puml\"}\n@startuml\nIGNORED\n@enduml\n:::\n");
        let mock = Mock::ok("<svg>FILE</svg>");
        let map = diagrams(&[("a.puml", "@startuml\nREAL\n@enduml")]);
        let _html = run(&inst, "", &map, Some(&mock), "d-2");
        // The renderer was handed the FILE source, not the inline body.
        assert_eq!(mock.calls.get(), 1);
    }

    #[test]
    fn missing_src_file_is_a_detailed_error_not_a_render() {
        let inst = one_directive(":::plantuml{src=\"nope.puml\"}\n:::\n");
        let mock = Mock::ok("<svg/>");
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-3");
        assert_eq!(
            mock.calls.get(),
            0,
            "must not call the server for a missing file"
        );
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("not found"));
        assert!(html.contains("nope.puml"));
    }

    #[test]
    fn src_escaping_docs_root_is_rejected() {
        let inst = one_directive(":::plantuml{src=\"../../etc/passwd.puml\"}\n:::\n");
        let mock = Mock::ok("<svg/>");
        let html = run(&inst, "guide", &diagrams(&[]), Some(&mock), "d-4");
        assert_eq!(mock.calls.get(), 0);
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("escapes the docs root"));
    }

    #[test]
    fn empty_directive_reports_missing_source() {
        let inst = one_directive(":::plantuml\n:::\n");
        let mock = Mock::ok("<svg/>");
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-5");
        assert_eq!(mock.calls.get(), 0);
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("needs a"));
    }

    #[test]
    fn disabled_when_no_renderer() {
        let inst = one_directive(":::plantuml\n@startuml\nA->B\n@enduml\n:::\n");
        let html = run(&inst, "", &diagrams(&[]), None, "d-6");
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("disabled"));
    }

    #[test]
    fn unreachable_error_shows_server_and_detail() {
        let inst = one_directive(":::plantuml\n@startuml\nA->B\n@enduml\n:::\n");
        let mock = Mock::err(PlantumlError::Unreachable {
            server: "http://localhost:8080".into(),
            detail: "connection refused".into(),
        });
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-7");
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("http://localhost:8080"));
        assert!(html.contains("connection refused"));
    }

    #[test]
    fn server_syntax_error_shows_message_and_line() {
        let inst = one_directive(":::plantuml\n@startuml\nbroken\n@enduml\n:::\n");
        let mock = Mock::err(PlantumlError::Server {
            status: 400,
            message: "Syntax Error?".into(),
            line: Some(2),
        });
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-8");
        assert!(html.contains("docgen-plantuml-error"));
        assert!(html.contains("HTTP 400"));
        assert!(html.contains("line 2"));
        assert!(html.contains("Syntax Error?"));
    }

    #[test]
    fn error_reason_is_html_escaped() {
        let inst = one_directive(":::plantuml\n@startuml\nx\n@enduml\n:::\n");
        let mock = Mock::err(PlantumlError::Server {
            status: 400,
            message: "<script>alert(1)</script>".into(),
            line: None,
        });
        let html = run(&inst, "", &diagrams(&[]), Some(&mock), "d-9");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }
}
