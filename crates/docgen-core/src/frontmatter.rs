use serde_yml::Value;

/// Result of splitting frontmatter from a markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub frontmatter: Value,
    pub body: String,
}

/// Split an optional leading `---`-delimited YAML frontmatter block from the body.
/// On malformed YAML, frontmatter is `Value::Null` and the whole input is the body.
/// Handles both LF and CRLF line endings, empty frontmatter blocks, and requires the
/// closing fence to be a line containing exactly `---` (ignoring trailing whitespace).
pub fn parse_frontmatter(raw: &str) -> Parsed {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    // Match an opening `---` fence followed by a line break (LF or CRLF).
    let after_open = input
        .strip_prefix("---\n")
        .or_else(|| input.strip_prefix("---\r\n"));

    if let Some(rest) = after_open {
        // Walk line by line looking for a closing fence that is exactly `---`.
        let mut offset = 0usize;
        for line in rest.split_inclusive('\n') {
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            if trimmed.trim_end() == "---" {
                let yaml = &rest[..offset];
                let after = &rest[offset + line.len()..];
                let frontmatter = serde_yml::from_str(yaml).unwrap_or(Value::Null);
                return Parsed {
                    frontmatter,
                    body: after.to_string(),
                };
            }
            offset += line.len();
        }
        // Also handle a closing fence with no trailing newline (EOF).
        let last = &rest[offset..];
        if last.trim_end_matches('\r').trim_end() == "---" {
            let yaml = &rest[..offset];
            let frontmatter = serde_yml::from_str(yaml).unwrap_or(Value::Null);
            return Parsed {
                frontmatter,
                body: String::new(),
            };
        }
    }

    Parsed {
        frontmatter: Value::Null,
        body: input.to_string(),
    }
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
    fn parses_crlf_frontmatter() {
        let raw = "---\r\ntitle: X\r\n---\r\nbody\r\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("X"));
        assert_eq!(parsed.body, "body\r\n");
    }

    #[test]
    fn parses_empty_frontmatter_block() {
        let raw = "---\n---\nbody\n";
        let parsed = parse_frontmatter(raw);
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, "body\n");
    }

    #[test]
    fn longer_dash_run_is_not_a_closing_fence() {
        // A `----` line is not a bare `---` fence; it must not be treated as the close,
        // and no stray dash should leak into the body.
        let raw = "---\ntitle: X\n----\nbody\n";
        let parsed = parse_frontmatter(raw);
        // No valid closing fence -> whole input is body, frontmatter null.
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, raw);
    }

    #[test]
    fn malformed_yaml_falls_back_to_null_with_body() {
        let raw = "---\n: not: valid: yaml\n---\nbody\n";
        let parsed = parse_frontmatter(raw);
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, "body\n");
    }

    #[test]
    fn unterminated_block_returns_full_input_as_body() {
        let raw = "---\ntitle: X\n";
        let parsed = parse_frontmatter(raw);
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, raw);
    }

    #[test]
    fn strips_leading_bom() {
        let raw = "\u{feff}---\ntitle: X\n---\nbody\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("X"));
        assert_eq!(parsed.body, "body\n");
    }
}
