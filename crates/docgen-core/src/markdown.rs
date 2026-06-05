use comrak::{markdown_to_html, Options};

/// Render a markdown body (frontmatter already stripped) to HTML with GFM extensions.
pub fn render_markdown(body: &str) -> String {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    markdown_to_html(body, &options)
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
}
