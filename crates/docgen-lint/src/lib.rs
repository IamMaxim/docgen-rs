//! The docgen linter framework: severities + diagnostics ([`model`]), the
//! site-wide read model rules consume ([`context`]), the engine that discovers
//! the site, resolves configured severities and runs every rule ([`engine`]),
//! the rule registry ([`rules`]) and the output formatters ([`format`]).
//!
//! Rules themselves live in [`rules`] and are appended over time; this crate
//! provides the machinery. The engine reuses the exact discovery/prepare/extract
//! path the site build runs (docgen-core), so what the linter sees is what the
//! build renders.

pub mod context;
pub mod engine;
pub mod format;
pub mod model;
pub mod rules;

pub use context::{DocEntry, LintContext};
pub use engine::{list_rules, run, LintError, LintOptions, LintOutcome};
pub use model::{Diagnostic, Severity};
pub use rules::Rule;
