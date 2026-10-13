//! The expression gate for bundle export and cloud publish (PRD 17.1). Expressions run
//! only in the expression engine, never in the backend commands, so the check happens in
//! the UI: the Problems view registers the document-wide expression check here, and
//! export and publish refuse to continue while it reports errors.
//! Port of expressionBlocker in src/shell/expressionIssues.ts.

use crate::archive::DocumentConfig;
use std::sync::Mutex;

/// Counts the expression errors of a definition.
pub type ExpressionGate = fn(&DocumentConfig) -> usize;

static GATE: Mutex<Option<ExpressionGate>> = Mutex::new(None);

/// Registers the document-wide expression check (the Problems view does this once).
pub fn set_expression_gate(gate: Option<ExpressionGate>) {
    *GATE.lock().unwrap_or_else(|e| e.into_inner()) = gate;
}

/// How many expressions of `config` do not check; 0 until a check is registered.
pub fn expression_error_count(config: &DocumentConfig) -> usize {
    let gate = *GATE.lock().unwrap_or_else(|e| e.into_inner());
    gate.map_or(0, |gate| gate(config))
}

/// Plain-language blocker shown by bundle export and cloud publish.
pub fn expression_blocker(count: usize) -> String {
    format!(
        "{count} expression error{} must be fixed first (see Settings › Problems)",
        if count == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::{expression_blocker, expression_error_count, set_expression_gate};
    use crate::archive::DocumentConfig;

    #[test]
    fn words_the_blocker_by_count() {
        assert_eq!(
            expression_blocker(1),
            "1 expression error must be fixed first (see Settings › Problems)"
        );
        assert!(expression_blocker(3).starts_with("3 expression errors "));
    }

    #[test]
    fn counts_zero_until_a_check_is_registered() {
        let config = DocumentConfig::default();
        assert_eq!(expression_error_count(&config), 0);
        set_expression_gate(Some(|_| 2));
        assert_eq!(expression_error_count(&config), 2);
        set_expression_gate(None);
        assert_eq!(expression_error_count(&config), 0);
    }
}
