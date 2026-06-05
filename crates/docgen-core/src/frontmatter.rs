use serde_yml::Value;

/// Result of splitting frontmatter from a markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub frontmatter: Value,
    pub body: String,
}

/// Split an optional leading `---`-delimited YAML frontmatter block from the body.
/// On malformed YAML, frontmatter is `Value::Null` and the whole input is the body.
pub fn parse_frontmatter(raw: &str) -> Parsed {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    if let Some(rest) = input.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            // Skip past the closing `\n---`, then a trailing newline if present.
            let after = &rest[end + "\n---".len()..];
            let body = after.strip_prefix('\n').unwrap_or(after);
            let frontmatter = serde_yml::from_str(yaml).unwrap_or(Value::Null);
            return Parsed { frontmatter, body: body.to_string() };
        }
    }

    Parsed { frontmatter: Value::Null, body: input.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_frontmatter_and_body() {
        let raw = "---\ntitle: Hello\n---\n# Body\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("Hello"));
        assert_eq!(parsed.body, "# Body\n");
    }

    #[test]
    fn no_frontmatter_returns_null_and_full_body() {
        let raw = "# Just body\n";
        let parsed = parse_frontmatter(raw);
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, "# Just body\n");
    }

    #[test]
    fn strips_leading_bom() {
        let raw = "\u{feff}---\ntitle: X\n---\nbody\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("X"));
        assert_eq!(parsed.body, "body\n");
    }
}
