//! The rule registry. A rule inspects the site-wide [`LintContext`] and emits
//! [`Diagnostic`]s at its *default* severity; the engine re-levels them to the
//! configured severity afterward, so rules never read `[lint.rules]` themselves.

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};

/// One lint rule.
pub trait Rule {
    /// Stable kebab-case id (config key in `[lint.rules]`).
    fn id(&self) -> &'static str;
    /// Severity used when `[lint.rules]` has no override for this rule.
    fn default_severity(&self) -> Severity;
    /// One-line description, for `--list-rules`.
    fn description(&self) -> &'static str;
    /// Emit findings into `out` with `severity = self.default_severity()`;
    /// the engine re-levels them to the resolved severity.
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>);
}

/// Every built-in rule, in the order they run. Empty for now: rules land in
/// later milestones and are appended here.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    Vec::new()
}
