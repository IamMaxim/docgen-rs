use std::sync::OnceLock;

use comrak::nodes::AstNode;
use comrak::options::Plugins;
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{format_html_with_plugins, markdown_to_html_with_plugins, Options};

/// Default syntect theme. Single source of truth.
pub const SYNTECT_THEME: &str = "InspiredGitHub";

/// The syntect adapter loads/builds syntect's syntax + theme sets, which is the
/// single most expensive object in the pipeline. It is immutable and reusable, so
/// build it once and share `&adapter` across every document.
fn syntect_adapter() -> &'static SyntectAdapter {
    static ADAPTER: OnceLock<SyntectAdapter> = OnceLock::new();
    ADAPTER.get_or_init(|| SyntectAdapter::new(Some(SYNTECT_THEME)))
}

/// The comrak options used across the whole pipeline (GFM + P0 extensions).
/// Single source of truth so the AST pass (Cluster B) and the one-shot render agree.
pub fn comrak_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    // Allow raw inline HTML through: the wikilink AST pass injects `HtmlInline`
    // nodes (resolved anchors / broken spans) that must render, not be omitted.
    options.render.r#unsafe = true;
    options
}

/// Render a markdown body (frontmatter already stripped) to HTML with GFM
/// extensions and server-side syntect syntax highlighting of fenced code.
pub fn render_markdown(body: &str) -> String {
    let options = comrak_options();
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(syntect_adapter());
    markdown_to_html_with_plugins(body, &options, &plugins)
}

/// Format an already-parsed (and possibly transformed) AST to HTML with syntect.
pub fn format_ast<'a>(root: &'a AstNode<'a>, options: &Options) -> String {
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(syntect_adapter());
    let mut out = String::new();
    format_html_with_plugins(root, options, &mut out, &plugins).expect("format AST to HTML");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_to_html() {
        let html = render_markdown("# Title");
        assert!(html.contains("<h1>"));
        assert!(html.contains("Title"));
    }

    #[test]
    fn renders_gfm_table() {
        let md = "| a | b |\n| - | - |\n| 1 | 2 |\n";
        let html = render_markdown(md);
        assert!(html.contains("<table>"));
    }

    #[test]
    fn renders_strikethrough() {
        let html = render_markdown("~~gone~~");
        assert!(html.contains("<del>"));
    }

    #[test]
    fn renders_task_list() {
        let html = render_markdown("- [x] done\n- [ ] todo\n");
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("checked"));
    }

    #[test]
    fn renders_autolink() {
        let html = render_markdown("see https://example.com here\n");
        assert!(html.contains(r#"href="https://example.com""#));
    }

    #[test]
    fn renders_footnote() {
        let html = render_markdown("text[^1]\n\n[^1]: a note\n");
        assert!(html.contains("<sup"));
        assert!(html.contains("footnote"));
    }

    #[test]
    fn highlights_fenced_rust_code() {
        let md = "```rust\nfn main() { let x = 1; }\n```\n";
        let html = render_markdown(md);
        // Syntect emits inline-styled spans inside a <pre> wrapper.
        assert!(html.contains("<pre"));
        assert!(html.contains("style=\"color:"));
        // The keyword `fn` is highlighted as its own span, not left as plain text.
        assert!(html.contains("<span"));
    }

    #[test]
    fn unknown_language_does_not_crash_and_still_wraps_pre() {
        let md = "```not-a-real-lang\nplain text\n```\n";
        let html = render_markdown(md);
        assert!(html.contains("<pre"));
        assert!(html.contains("plain text"));
    }

    #[test]
    fn comrak_options_is_shared_source_of_truth() {
        // The shared options keep the P0 GFM extensions on.
        let html = render_markdown("~~gone~~\n");
        assert!(html.contains("<del>"));
    }
}
