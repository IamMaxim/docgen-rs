//! Small shared helpers used by the AST passes.

/// Escape the five HTML-significant characters so untrusted text (math source,
/// diagram source) can be embedded inside generated markup safely.
pub fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_angle_brackets_and_amp() {
        assert_eq!(escape_html("a<b>&\"c\""), "a&lt;b&gt;&amp;&quot;c&quot;");
    }
}
