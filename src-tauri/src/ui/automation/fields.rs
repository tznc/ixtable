//! Expression and number checks of the automation editors (port of
//! `src/automation/fields.tsx`). The controls themselves live in `ui::controls`.

/// Root names an action or trigger expression may use.
pub const AUTOMATION_SCOPE_NAMES: [&str; 8] =
    ["record", "old", "form", "app", "params", "results", "steps", "trigger"];

/// The first problem of an expression, if any (`required` makes an empty one a problem).
pub fn expression_problem(value: &str, required: bool) -> Option<String> {
    if value.trim().is_empty() {
        return required.then(|| "Expression is required".to_string());
    }
    crate::expr::check(value, Some(&AUTOMATION_SCOPE_NAMES))
        .into_iter()
        .next()
        .map(|d| d.message)
}

/// The problem of a whole-number input (`None` when `text` is a whole number >= `min`).
pub fn number_problem(text: &str, min: i64) -> Option<String> {
    match parse_whole(text, min) {
        Some(_) => None,
        None => Some(format!("Enter a whole number of at least {min}")),
    }
}

/// `text` as a whole number of at least `min`.
pub fn parse_whole(text: &str, min: i64) -> Option<i64> {
    text.trim().parse::<i64>().ok().filter(|n| *n >= min)
}

#[cfg(test)]
mod tests {
    use super::{expression_problem, number_problem, parse_whole};

    #[test]
    fn empty_expressions_are_a_problem_only_when_required() {
        assert_eq!(expression_problem("  ", true).as_deref(), Some("Expression is required"));
        assert_eq!(expression_problem("", false), None);
    }

    #[test]
    fn expressions_report_their_first_diagnostic() {
        assert_eq!(
            expression_problem("'closed' +", true).as_deref(),
            Some("Unexpected end of expression")
        );
        assert_eq!(expression_problem("record.status = 'open'", true), None);
        assert!(expression_problem("nope.value", true).is_some(), "unknown root name");
    }

    #[test]
    fn whole_numbers_respect_the_minimum() {
        assert_eq!(parse_whole(" 3 ", 1), Some(3));
        assert_eq!(parse_whole("0", 1), None);
        assert_eq!(parse_whole("1.5", 0), None);
        assert_eq!(parse_whole("", 0), None);
        assert_eq!(number_problem("x", 1).as_deref(), Some("Enter a whole number of at least 1"));
        assert_eq!(number_problem("600000", 0), None);
    }
}
