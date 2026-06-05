//! Source-level directive pre/post pass. `extract` rewrites raw markdown,
//! replacing each directive with an HTML-comment sentinel and returning the
//! parsed instances; `substitute` swaps sentinels for rendered component HTML
//! after comrak has formatted the surrounding markdown.
//!
//! Why a source-level pre-pass and not a comrak AST pass: comrak 0.52 has no
//! generic `:::` directive extension, and a block directive's inner content must
//! itself be parsed as markdown. Reconstructing block boundaries from a flattened
//! inline AST is fragile and loses the raw inner-markdown span we need. Operating
//! on the raw body string before `parse_document` keeps the directive system
//! orthogonal to comrak's AST passes (wikilink/math/mermaid still run on the
//! rewritten source) and yields the verbatim inner-markdown span block directives
//! require. The sentinel is an HTML comment so comrak passes it through verbatim
//! (with `render.unsafe = true`); a post-pass substitutes the rendered HTML.

use std::collections::BTreeMap;

/// One directive found in a doc body.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveInstance {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    /// Leaf `[label]`; empty for block form.
    pub label: String,
    /// Block inner markdown; empty for leaf form.
    pub inner_md: String,
    pub is_block: bool,
}

/// The sentinel a directive is replaced with in the rewritten source. `idx` is
/// the instance index. An HTML comment so comrak passes it through verbatim.
fn sentinel(idx: usize) -> String {
    format!("<!--docgen-directive:{idx}-->")
}

/// True if `c` may start/continue a directive name (`[A-Za-z][A-Za-z0-9_-]*`).
fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}
fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Parse an attr string (`type=warning title="x y" wide`) → ordered map. Total:
/// malformed input degrades gracefully (best-effort token split), never panics.
/// A bare key (`wide`) becomes `wide="true"`.
pub fn parse_attrs(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Skip whitespace between tokens.
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        // Read a key: up to `=` or whitespace.
        let key_start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect();
        if key.is_empty() {
            i += 1;
            continue;
        }
        // Bare key (no `=`): value "true".
        if i >= chars.len() || chars[i] != '=' {
            out.insert(key, "true".to_string());
            continue;
        }
        // Consume `=` and read the value (quoted or bare).
        i += 1; // skip '='
        let value = if i < chars.len() && chars[i] == '"' {
            i += 1; // skip opening quote
            let v_start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let v: String = chars[v_start..i].iter().collect();
            if i < chars.len() {
                i += 1; // skip closing quote
            }
            v
        } else {
            let v_start = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            chars[v_start..i].iter().collect()
        };
        out.insert(key, value);
    }
    out
}

/// Parse a `:::<name>{attrs}` open fence line (already trimmed). Returns
/// `(name, attrs_str)` on success.
fn parse_block_open(trimmed: &str) -> Option<(String, String)> {
    let rest = trimmed.strip_prefix(":::")?;
    let mut chars = rest.char_indices();
    let (first_i, first) = chars.next()?;
    debug_assert_eq!(first_i, 0);
    if !is_name_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in rest.char_indices().skip(1) {
        if is_name_char(c) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    let name = &rest[..end];
    let after = rest[end..].trim();
    // After the name, only an optional `{...}` attr block (and nothing else).
    let attrs = if after.is_empty() {
        String::new()
    } else if after.starts_with('{') && after.ends_with('}') {
        after[1..after.len() - 1].to_string()
    } else {
        return None;
    };
    Some((name.to_string(), attrs))
}

/// Pass 1: scan `body_md`, replace directives with sentinels, return instances
/// (index-aligned with the sentinels). Unknown-vs-known is NOT decided here —
/// every syntactic directive is extracted; resolution happens in `substitute`.
pub fn extract(body_md: &str) -> (String, Vec<DirectiveInstance>) {
    let mut instances: Vec<DirectiveInstance> = Vec::new();
    let mut out_lines: Vec<String> = Vec::new();

    let lines: Vec<&str> = body_md.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Escaped directive opener: `\:::name...` → emit literal, drop backslash.
        if let Some(rest) = trimmed.strip_prefix('\\') {
            if rest.starts_with(":::") || (rest.starts_with(':') && looks_like_leaf(rest)) {
                let indent = &line[..line.len() - line.trim_start().len()];
                out_lines.push(format!("{indent}{rest}"));
                i += 1;
                continue;
            }
        }

        // Block directive open?
        if let Some((name, attrs_str)) = parse_block_open(trimmed) {
            // Collect inner lines until the matching `:::` close (depth-counted).
            let mut depth = 1;
            let mut inner: Vec<&str> = Vec::new();
            let mut j = i + 1;
            let mut closed = false;
            while j < lines.len() {
                let t = lines[j].trim();
                if t == ":::" {
                    depth -= 1;
                    if depth == 0 {
                        closed = true;
                        break;
                    }
                    inner.push(lines[j]);
                } else if parse_block_open(t).is_some() {
                    depth += 1;
                    inner.push(lines[j]);
                } else {
                    inner.push(lines[j]);
                }
                j += 1;
            }
            if closed {
                let idx = instances.len();
                instances.push(DirectiveInstance {
                    name,
                    attrs: parse_attrs(&attrs_str),
                    label: String::new(),
                    inner_md: inner.join("\n"),
                    is_block: true,
                });
                out_lines.push(sentinel(idx));
                i = j + 1; // skip past the closing `:::`
                continue;
            }
            // Unterminated block: fall through, treat line as ordinary text.
        }

        // Otherwise scan the line for inline leaf directives.
        out_lines.push(scan_leaf_line(line, &mut instances));
        i += 1;
    }

    (out_lines.join("\n"), instances)
}

/// Heuristic for the escape branch: does `rest` (after a leading `:`) look like a
/// leaf directive `name[...]` or `name{...}`?
fn looks_like_leaf(rest: &str) -> bool {
    let body = &rest[1..];
    let name_len = body
        .char_indices()
        .take_while(|(k, c)| {
            if *k == 0 {
                is_name_start(*c)
            } else {
                is_name_char(*c)
            }
        })
        .map(|(_, c)| c.len_utf8())
        .sum::<usize>();
    if name_len == 0 {
        return false;
    }
    matches!(body[name_len..].chars().next(), Some('[') | Some('{'))
}

/// Replace every inline `:name[label]{attrs}` leaf directive in `line` with its
/// sentinel, appending instances. A `:::` block opener is never matched here
/// (block openers are handled before this is called, and a `::` prefix is
/// skipped). Plain `:` in prose (`10:30`) is left untouched.
fn scan_leaf_line(line: &str, instances: &mut Vec<DirectiveInstance>) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ':' {
            // Not a leaf if preceded or followed by another colon (`::`).
            let prev_colon = i > 0 && chars[i - 1] == ':';
            let next_colon = i + 1 < chars.len() && chars[i + 1] == ':';
            if !prev_colon && !next_colon {
                if let Some((inst, consumed)) = try_parse_leaf(&chars, i) {
                    let idx = instances.len();
                    instances.push(inst);
                    out.push_str(&sentinel(idx));
                    i += consumed;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Try to parse a leaf directive starting at `chars[start] == ':'`. Returns the
/// instance and the number of chars consumed (including the leading `:`).
fn try_parse_leaf(chars: &[char], start: usize) -> Option<(DirectiveInstance, usize)> {
    let mut i = start + 1; // skip ':'
    if i >= chars.len() || !is_name_start(chars[i]) {
        return None;
    }
    let name_start = i;
    while i < chars.len() && is_name_char(chars[i]) {
        i += 1;
    }
    let name: String = chars[name_start..i].iter().collect();

    // Leaf form requires a `[label]` immediately after the name.
    if i >= chars.len() || chars[i] != '[' {
        return None;
    }
    i += 1; // skip '['
    let label_start = i;
    while i < chars.len() && chars[i] != ']' {
        i += 1;
    }
    if i >= chars.len() {
        return None; // unterminated label
    }
    let label: String = chars[label_start..i].iter().collect();
    i += 1; // skip ']'

    // Optional `{attrs}`.
    let mut attrs = BTreeMap::new();
    if i < chars.len() && chars[i] == '{' {
        i += 1; // skip '{'
        let a_start = i;
        while i < chars.len() && chars[i] != '}' {
            i += 1;
        }
        if i >= chars.len() {
            return None; // unterminated attrs
        }
        let attrs_str: String = chars[a_start..i].iter().collect();
        attrs = parse_attrs(&attrs_str);
        i += 1; // skip '}'
    }

    Some((
        DirectiveInstance {
            name,
            attrs,
            label,
            inner_md: String::new(),
            is_block: false,
        },
        i - start,
    ))
}

/// Pass 2: replace each `<!--docgen-directive:N-->` sentinel in `html` with the
/// component's rendered HTML. `render_inner` renders a block directive's inner
/// markdown to HTML (the full pipeline, recursively). Returns the substituted
/// HTML and the set of component names that were actually rendered (for per-page
/// island/style gating). An unknown directive (or a component whose template
/// errors) becomes a clearly-marked inert error span — never a crash.
pub fn substitute(
    html: &str,
    instances: &[DirectiveInstance],
    registry: &docgen_components::Registry,
    render_inner: &dyn Fn(&str) -> String,
) -> (String, std::collections::BTreeSet<String>) {
    use docgen_components::DirectiveContext;
    let mut used = std::collections::BTreeSet::new();
    let mut out = html.to_string();
    for (idx, inst) in instances.iter().enumerate() {
        let rendered = match registry.get(&inst.name) {
            Some(component) => {
                let content = if inst.is_block {
                    render_inner(&inst.inner_md)
                } else {
                    String::new()
                };
                let ctx = DirectiveContext {
                    attrs: inst.attrs.clone(),
                    content,
                    label: inst.label.clone(),
                    id: format!("docgen-d-{idx}"),
                };
                match component.render(&ctx) {
                    Ok(h) => {
                        used.insert(inst.name.clone());
                        h
                    }
                    Err(_) => error_span(&inst.name, "template error"),
                }
            }
            None => error_span(&inst.name, "unknown directive"),
        };
        out = out.replace(&sentinel(idx), &rendered);
    }
    (out, used)
}

/// An inert, clearly-marked error span for an unresolved/failed directive. The
/// directive name is HTML-escaped so a malformed name cannot inject markup.
fn error_span(name: &str, reason: &str) -> String {
    let safe = crate::util::escape_html(name);
    format!(
        "<span class=\"docgen-directive-error\" data-directive=\"{safe}\">[docgen: {reason} `{safe}`]</span>"
    )
}

#[cfg(test)]
mod substitute_tests {
    use super::*;

    fn reg_with(name: &str, tpl: &str) -> docgen_components::Registry {
        let mut r = docgen_components::Registry::empty();
        r.insert(docgen_components::Component::from_parts(name, tpl, None, None));
        r
    }

    #[test]
    fn substitutes_known_block_component_and_renders_inner() {
        let (html, inst) = extract(":::callout{type=note}\n**hi**\n:::\n");
        let reg = reg_with(
            "callout",
            "<aside class=\"c--{{ attrs.type }}\">{{ content | safe }}</aside>",
        );
        let render_inner = |md: &str| format!("<p>{}</p>", md.trim().replace("**", ""));
        let (out, used) = substitute(&html, &inst, &reg, &render_inner);
        assert!(out.contains("c--note"));
        assert!(out.contains("<p>hi</p>"));
        assert!(used.contains("callout"));
        assert!(!out.contains("docgen-directive:")); // sentinel gone
    }

    #[test]
    fn unknown_directive_becomes_marked_error_span_not_panic() {
        let (html, inst) = extract(":bogus[x]{}\n");
        let reg = docgen_components::Registry::empty();
        let (out, used) = substitute(&html, &inst, &reg, &|s| s.to_string());
        assert!(out.contains("docgen-directive-error"));
        assert!(out.contains("unknown directive"));
        assert!(out.contains("bogus"));
        assert!(used.is_empty());
    }

    /// Build a doc that is just the sentinel for instance 0.
    fn sentinel_doc() -> String {
        format!("before {} after", sentinel(0))
    }

    #[test]
    fn directive_name_in_error_is_escaped() {
        // Craft an instance with a name that contains markup to exercise escaping.
        let inst = vec![DirectiveInstance {
            name: "<img>".into(),
            attrs: Default::default(),
            label: String::new(),
            inner_md: String::new(),
            is_block: false,
        }];
        let html = sentinel_doc();
        let (out, _) = substitute(
            &html,
            &inst,
            &docgen_components::Registry::empty(),
            &|s| s.to_string(),
        );
        assert!(out.contains("&lt;img&gt;"));
        assert!(!out.contains("<img>"));
    }

    #[test]
    fn template_error_becomes_error_span_not_panic() {
        // A template referencing an undefined filter fails to render.
        let reg = reg_with("boom", "{{ content | nonexistent_filter }}");
        let (html, inst) = extract(":::boom{}\nx\n:::\n");
        let (out, used) = substitute(&html, &inst, &reg, &|s| s.to_string());
        assert!(out.contains("docgen-directive-error"));
        assert!(out.contains("template error"));
        assert!(used.is_empty());
    }
}

#[cfg(test)]
mod extract_tests {
    use super::*;

    #[test]
    fn parse_attrs_handles_bare_quoted_and_empty() {
        let a = parse_attrs("type=warning title=\"Back up first\" wide");
        assert_eq!(a.get("type").unwrap(), "warning");
        assert_eq!(a.get("title").unwrap(), "Back up first");
        assert_eq!(a.get("wide").unwrap(), "true");
        assert!(parse_attrs("").is_empty());
    }

    #[test]
    fn extracts_block_directive_with_inner_markdown() {
        let src = ":::callout{type=warning title=\"Heads up\"}\nThis is **bold**.\n:::\n";
        let (out, inst) = extract(src);
        assert_eq!(inst.len(), 1);
        assert!(inst[0].is_block);
        assert_eq!(inst[0].name, "callout");
        assert_eq!(inst[0].attrs.get("type").unwrap(), "warning");
        assert_eq!(inst[0].inner_md.trim(), "This is **bold**.");
        assert!(out.contains("<!--docgen-directive:0-->"));
        assert!(!out.contains(":::"));
    }

    #[test]
    fn extracts_leaf_directive_with_label_and_attrs() {
        let src = "See :youtube[Intro]{id=abc123} now.\n";
        let (out, inst) = extract(src);
        assert_eq!(inst.len(), 1);
        assert!(!inst[0].is_block);
        assert_eq!(inst[0].name, "youtube");
        assert_eq!(inst[0].label, "Intro");
        assert_eq!(inst[0].attrs.get("id").unwrap(), "abc123");
        assert!(out.contains("See <!--docgen-directive:0--> now."));
    }

    #[test]
    fn nested_block_directives_match_outermost() {
        let src =
            ":::callout{type=note}\nouter\n:::callout{type=warning}\ninner\n:::\n:::\n";
        let (_out, inst) = extract(src);
        assert_eq!(inst.len(), 1); // only the outer is extracted at this level
        assert!(inst[0].inner_md.contains(":::callout{type=warning}"));
        assert!(inst[0].inner_md.contains("inner"));
    }

    #[test]
    fn escaped_directive_is_left_literal() {
        let src = "\\:::callout{}\nnot a directive\n:::\n";
        let (out, inst) = extract(src);
        assert!(inst.is_empty());
        assert!(out.contains(":::callout{}")); // literal, backslash removed
    }

    #[test]
    fn plain_text_with_colons_is_not_a_directive() {
        let src = "time is 10:30 and ratio 3:4\n";
        let (out, inst) = extract(src);
        assert!(inst.is_empty());
        assert_eq!(out, src);
    }
}
