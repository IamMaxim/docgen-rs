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
pub fn render_math(src: &str, display: bool) -> String {
    let opts = katex::Opts::builder()
        .display_mode(display)
        .throw_on_error(true)
        .build()
        .expect("katex opts build");
    match katex::render_with_opts(src, &opts) {
        Ok(html) => html,
        Err(_) => format!(
            "<code class=\"docgen-math-error\">{}</code>",
            escape_html(src)
        ),
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
        assert!(!html.contains("<script"));
    }
}
