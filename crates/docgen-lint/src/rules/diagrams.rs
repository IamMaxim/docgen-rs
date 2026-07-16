//! Diagram rules: PlantUML directive sources/bodies and mermaid fences.

use docgen_core::directivepass::{self, DirectiveInstance};
use docgen_core::pipeline::resolve_include_key;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::util::{doc_dir, line32};
use crate::rules::Rule;

/// `:::plantuml{src=…}` whose `.puml` file does not exist.
pub struct PlantumlSrcMissing;

impl Rule for PlantumlSrcMissing {
    fn id(&self) -> &'static str {
        "plantuml-src-missing"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "plantuml directive src does not name a known .puml file"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            let base_dir = doc_dir(&doc.prepared.rel_path);
            for d in doc.refs.directives.iter().filter(|d| d.name == "plantuml") {
                let Some(src) = d.src.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
                    continue; // inline-body form — `plantuml-empty`'s territory
                };
                let found = resolve_include_key(base_dir, src)
                    .is_some_and(|key| ctx.diagrams.contains_key(&key));
                if !found {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(d.line),
                        col: None,
                        message: format!("plantuml src `{src}` not found"),
                        note: None,
                    });
                }
            }
        }
    }
}

/// A plantuml directive with neither a `src` nor any inline body.
pub struct PlantumlEmpty;

/// Walk every directive instance in `body`, recursing into block bodies the
/// same way `extract_refs` does (so reported lines match the source file).
/// Shared with the external `plantuml-syntax` rule, which needs inline bodies.
pub(crate) fn walk_directives(
    body: &str,
    offset: usize,
    f: &mut dyn FnMut(&DirectiveInstance, usize),
) {
    let (_, instances) = directivepass::extract(body);
    for inst in &instances {
        f(inst, offset + inst.line);
        if inst.is_block && !inst.inner_md.is_empty() {
            walk_directives(&inst.inner_md, offset + inst.line, f);
        }
    }
}

impl Rule for PlantumlEmpty {
    fn id(&self) -> &'static str {
        "plantuml-empty"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "plantuml directive has no src and an empty body"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            // `DocRefs` doesn't retain block bodies, so re-run the (cheap)
            // directive pass to see whether the inline body is blank.
            walk_directives(&doc.prepared.body_md, 0, &mut |inst, line| {
                if inst.name != "plantuml" {
                    return;
                }
                let has_src = inst.attrs.get("src").is_some_and(|s| !s.trim().is_empty());
                if !has_src && inst.inner_md.trim().is_empty() {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(line),
                        col: None,
                        message: "plantuml directive has no src and an empty body".to_string(),
                        note: None,
                    });
                }
            });
        }
    }
}

/// A ```mermaid fence with a blank body.
pub struct MermaidEmpty;

impl Rule for MermaidEmpty {
    fn id(&self) -> &'static str {
        "mermaid-empty"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "mermaid fence has an empty body"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for f in doc.refs.fences.iter().filter(|f| f.lang == "mermaid") {
                if f.body.trim().is_empty() {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(f.line),
                        col: None,
                        message: "mermaid fence has an empty body".to_string(),
                        note: None,
                    });
                }
            }
        }
    }
}

/// A mermaid fence whose first keyword is not a known diagram type.
pub struct MermaidUnknownType;

/// Diagram-type keywords mermaid recognizes as the first word of a definition.
const MERMAID_TYPES: &[&str] = &[
    "graph",
    "flowchart",
    "sequenceDiagram",
    "classDiagram",
    "stateDiagram",
    "stateDiagram-v2",
    "erDiagram",
    "journey",
    "gantt",
    "pie",
    "quadrantChart",
    "requirementDiagram",
    "gitGraph",
    "C4Context",
    "C4Container",
    "C4Component",
    "C4Dynamic",
    "C4Deployment",
    "mindmap",
    "timeline",
    "zenuml",
    "sankey",
    "sankey-beta",
    "xychart",
    "xychart-beta",
    "block",
    "block-beta",
    "packet",
    "packet-beta",
    "kanban",
    "architecture",
    "architecture-beta",
    "radar",
];

/// The first meaningful word of a mermaid body: blank lines, `%%` comments and
/// (possibly multi-line) `%%{init}%%` blocks are skipped.
fn mermaid_type(body: &str) -> Option<&str> {
    let mut in_init = false;
    for line in body.lines() {
        let t = line.trim();
        if in_init {
            if t.contains("}%%") {
                in_init = false;
            }
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if t.starts_with("%%{") && !t.contains("}%%") {
            in_init = true;
            continue;
        }
        if t.starts_with("%%") {
            continue;
        }
        return t.split_whitespace().next();
    }
    None
}

impl Rule for MermaidUnknownType {
    fn id(&self) -> &'static str {
        "mermaid-unknown-type"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "mermaid fence does not start with a known diagram type"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for f in doc.refs.fences.iter().filter(|f| f.lang == "mermaid") {
                // A blank fence is `mermaid-empty`'s finding, not ours.
                let Some(kind) = mermaid_type(&f.body) else {
                    continue;
                };
                if !MERMAID_TYPES.contains(&kind) {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(f.line),
                        col: None,
                        message: format!("unknown mermaid diagram type `{kind}`"),
                        note: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mermaid_type;
    use crate::rules::test_fixture::lint_fixture;

    #[test]
    fn plantuml_src_missing_flags_absent_puml() {
        let diags = lint_fixture(
            &[
                (
                    "index.md",
                    ":::plantuml{src=d.puml}\n:::\n\n:::plantuml{src=gone.puml}\n:::\n",
                ),
                ("d.puml", "@startuml\nA -> B\n@enduml\n"),
            ],
            "plantuml-src-missing",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("gone.puml"));
        assert_eq!(diags[0].line, Some(4));
    }

    #[test]
    fn plantuml_src_missing_resolves_relative_to_the_doc() {
        let diags = lint_fixture(
            &[
                ("guide/index.md", ":::plantuml{src=../shared/d.puml}\n:::\n"),
                ("shared/d.puml", "@startuml\n@enduml\n"),
            ],
            "plantuml-src-missing",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn plantuml_empty_flags_srcless_bodyless_forms() {
        let diags = lint_fixture(
            &[(
                "index.md",
                ":::plantuml{}\n:::\n\n:::plantuml{}\nA -> B\n:::\n\n:::plantuml{src=d.puml}\n:::\n",
            )],
            "plantuml-empty",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, Some(1));
    }

    #[test]
    fn mermaid_empty_flags_blank_fences_only() {
        let diags = lint_fixture(
            &[("index.md", "```mermaid\n```\n\n```mermaid\ngraph TD\n```\n")],
            "mermaid-empty",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, Some(1));
    }

    #[test]
    fn mermaid_unknown_type_flags_bogus_first_word() {
        let diags = lint_fixture(
            &[(
                "index.md",
                "```mermaid\nfoo TD\n```\n\n```mermaid\nflowchart LR\n```\n",
            )],
            "mermaid-unknown-type",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("`foo`"));
    }

    #[test]
    fn mermaid_unknown_type_skips_comments_and_init_blocks() {
        let diags = lint_fixture(
            &[(
                "index.md",
                "```mermaid\n%% a comment\n%%{init: {'theme':'dark'}}%%\nsequenceDiagram\n```\n",
            )],
            "mermaid-unknown-type",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mermaid_type_handles_multiline_init_and_blank_bodies() {
        assert_eq!(
            mermaid_type("%%{init: {\n  'theme': 'dark'\n}}%%\npie\n"),
            Some("pie")
        );
        assert_eq!(mermaid_type("  \n%% only comments\n"), None);
        assert_eq!(mermaid_type("graph TD\nA-->B\n"), Some("graph"));
    }
}
