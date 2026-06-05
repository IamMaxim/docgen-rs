use comrak::options::Plugins;
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{markdown_to_html_with_plugins, Options};

/// Default syntect theme. Single source of truth.
pub const SYNTECT_THEME: &str = "InspiredGitHub";

/// The comrak options used across the whole pipeline (GFM + P0 extensions).
/// Single source of truth so the AST pass (Cluster B) and the one-shot render agree.
pub fn comrak_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options
}

/// Render a markdown body (frontmatter already stripped) to HTML with GFM
/// extensions and server-side syntect syntax highlighting of fenced code.
pub fn render_markdown(body: &str) -> String {
    let options = comrak_options();
    let adapter = SyntectAdapter::new(Some(SYNTECT_THEME));
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);
    markdown_to_html_with_plugins(body, &options, &plugins)
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
