//! Build-time KaTeX rendering. Each math expression in a document is rendered
//! to static HTML at build time via the `katex` crate (default `quick-js`
//! backend), so the generated site ships **zero runtime JS for math**.

use crate::util::escape_html;

/// Render one math expression to KaTeX HTML at build time.
///
/// `display` selects block (`$$`) vs inline (`$`) layout. On a KaTeX parse
/// error we fall back to an escaped `<code>` so a bad expression degrades
/// gracefully instead of failing the whole build.
///
/// `throw_on_error` is **true** here: with it false, KaTeX swallows invalid
/// input and emits its own red error markup (returning `Ok`), which would never
/// reach our graceful fallback. Letting KaTeX return `Err` on a genuine parse
/// failure lets us emit a clean escaped `<code class="docgen-math-error">`.
///
/// The error fallback honors `display`: a failed display (`$$`) equation is
/// wrapped in a block `<div class="katex-display docgen-math-error">` so it
/// still renders as a centered block (matching `.katex-display` spacing),
/// while inline math degrades to inline `<code>`. The KaTeX error message is
/// logged to stderr so a malformed expression leaves a build-time diagnostic.
pub fn render_math(src: &str, display: bool) -> String {
    let opts = katex::Opts::builder()
        .display_mode(display)
        .throw_on_error(true)
        .build()
        .expect("katex opts build");
    match katex::render_with_opts(src, &opts) {
        Ok(html) => html,
        Err(e) => {
            eprintln!("docgen: KaTeX failed to render math `{src}`: {e}");
            let escaped = escape_html(src);
            if display {
                format!("<div class=\"katex-display docgen-math-error\">{escaped}</div>")
            } else {
                format!("<code class=\"docgen-math-error\">{escaped}</code>")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_math_to_katex_html() {
        let html = render_math("E=mc^2", false);
        assert!(html.contains("katex"));
        assert!(!html.contains("katex-display")); // inline → no display wrapper
    }

    #[test]
    fn renders_display_math_with_display_wrapper() {
        let html = render_math("\\int_0^1 x\\,dx", true);
        assert!(html.contains("katex-display"));
    }

    #[test]
    fn bad_expression_degrades_to_escaped_code() {
        let html = render_math("\\frac{", false);
        assert!(html.contains("docgen-math-error"));
        assert!(html.contains("<code")); // inline → inline code
        assert!(!html.contains("<script"));
    }

    #[test]
    fn bad_display_expression_degrades_to_block() {
        // A failed *display* equation must stay a centered block, not collapse
        // to an inline <code> fragment.
        let html = render_math("\\frac{", true);
        assert!(html.contains("docgen-math-error"));
        assert!(html.contains("katex-display")); // block wrapper retained
        assert!(html.contains("<div")); // block element, not inline <code>
        assert!(!html.contains("<code"));
    }

    #[test]
    fn bad_expression_escapes_html_metacharacters() {
        // A malformed expression carrying HTML metacharacters must be escaped
        // before landing in raw HTML (render.unsafe = true downstream).
        let html = render_math("<script>\\frac{", false);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
    }
}
