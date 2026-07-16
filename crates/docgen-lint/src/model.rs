//! Core lint data types: [`Severity`] and [`Diagnostic`].

use std::fmt;
use std::str::FromStr;

/// How serious a finding is. Ordered: `Allow < Info < Warn < Error`, so the
/// engine can compare/promote levels directly. `Allow` silences a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Allow,
    Info,
    Warn,
    Error,
}

impl FromStr for Severity {
    type Err = String;

    /// Accepts `allow` (or its alias `off`), `info`, `warn`, `error` —
    /// case-insensitively, surrounding whitespace ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "allow" | "off" => Ok(Severity::Allow),
            "info" => Ok(Severity::Info),
            "warn" => Ok(Severity::Warn),
            "error" => Ok(Severity::Error),
            other => Err(other.to_string()),
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Allow => "allow",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        })
    }
}

impl serde::Serialize for Severity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// One lint finding. `file` is the docs-relative source path (e.g.
/// `guide/intro.md`); `line`/`col` are 1-based when known.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    /// The emitting rule's kebab-case id.
    pub rule: &'static str,
    pub severity: Severity,
    /// Docs-relative path of the file the finding is attributed to.
    pub file: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub message: String,
    /// Optional secondary hint rendered below the message.
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_accepts_all_levels_and_aliases() {
        assert_eq!("allow".parse::<Severity>().unwrap(), Severity::Allow);
        assert_eq!("off".parse::<Severity>().unwrap(), Severity::Allow);
        assert_eq!("info".parse::<Severity>().unwrap(), Severity::Info);
        assert_eq!("warn".parse::<Severity>().unwrap(), Severity::Warn);
        assert_eq!("error".parse::<Severity>().unwrap(), Severity::Error);
        // Tolerant of case + whitespace.
        assert_eq!(" Error ".parse::<Severity>().unwrap(), Severity::Error);
    }

    #[test]
    fn from_str_rejects_unknown_severity() {
        assert!("loud".parse::<Severity>().is_err());
        assert!("".parse::<Severity>().is_err());
    }

    #[test]
    fn ordering_ranks_error_highest() {
        assert!(Severity::Allow < Severity::Info);
        assert!(Severity::Info < Severity::Warn);
        assert!(Severity::Warn < Severity::Error);
    }

    #[test]
    fn display_is_lowercase_and_round_trips() {
        for sev in [
            Severity::Allow,
            Severity::Info,
            Severity::Warn,
            Severity::Error,
        ] {
            assert_eq!(sev.to_string().parse::<Severity>().unwrap(), sev);
        }
        assert_eq!(Severity::Warn.to_string(), "warn");
    }
}
