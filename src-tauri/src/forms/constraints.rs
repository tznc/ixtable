//! Resize limits per control kind (port of `src/design/constraints.ts`).

use crate::design::ControlKind;
use crate::ui::grid::engine::SpanConstraints;

/// Smallest useful width in columns per control kind, authored against a 12-column grid.
fn minimum(kind: ControlKind) -> i64 {
    match kind {
        ControlKind::Label | ControlKind::Boolean | ControlKind::Button => 1,
        ControlKind::Text
        | ControlKind::Number
        | ControlKind::Decimal
        | ControlKind::Date
        | ControlKind::Time
        | ControlKind::Select
        | ControlKind::Computed
        | ControlKind::Image => 2,
        ControlKind::Multiline
        | ControlKind::Datetime
        | ControlKind::Relationship
        | ControlKind::Attachment
        | ControlKind::MultiSelect => 3,
        ControlKind::Section | ControlKind::Tabs | ControlKind::RichText => 4,
        ControlKind::RelatedList => 6,
    }
}

/// Resize limits for a control on a grid with `columns` columns. Minimums scale with the
/// column count (never below 1, never above the grid), and the maximum is the full width.
pub fn control_constraints(kind: ControlKind, columns: usize) -> SpanConstraints {
    let count = (columns as i64).max(1);
    let scaled = ((minimum(kind) * count) as f64 / 12.0).round() as i64;
    SpanConstraints {
        min_column_span: Some(count.min(scaled.max(1))),
        max_column_span: Some(count),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::control_constraints;
    use crate::design::{GridLayout, Placement};
    use crate::forms::schema::CONTROL_KINDS;
    use crate::design::ControlKind;
    use crate::ui::grid::engine::{GridDelta, SpanConstraints, resize_placement};

    fn limits(min: i64, max: i64) -> (Option<i64>, Option<i64>) {
        (Some(min), Some(max))
    }
    fn of(c: SpanConstraints) -> (Option<i64>, Option<i64>) {
        (c.min_column_span, c.max_column_span)
    }

    #[test]
    fn gives_each_kind_a_minimum_width_and_caps_every_kind_at_the_grid_width() {
        assert_eq!(of(control_constraints(ControlKind::Boolean, 12)), limits(1, 12));
        assert_eq!(control_constraints(ControlKind::Text, 12).min_column_span, Some(2));
        assert_eq!(control_constraints(ControlKind::Section, 12).min_column_span, Some(4));
        assert_eq!(control_constraints(ControlKind::Tabs, 12).min_column_span, Some(4));
        assert_eq!(
            control_constraints(ControlKind::RelatedList, 12).min_column_span,
            Some(6)
        );
        for kind in CONTROL_KINDS {
            let l = control_constraints(kind, 12);
            assert_eq!(l.max_column_span, Some(12));
            let min = l.min_column_span.unwrap();
            assert!((1..=12).contains(&min));
        }
    }

    #[test]
    fn scales_minimums_with_the_column_count_and_never_exceeds_a_narrow_grid() {
        assert_eq!(of(control_constraints(ControlKind::RelatedList, 4)), limits(2, 4));
        assert_eq!(control_constraints(ControlKind::Boolean, 4).min_column_span, Some(1));
        assert_eq!(of(control_constraints(ControlKind::RelatedList, 1)), limits(1, 1));
        assert_eq!(of(control_constraints(ControlKind::Tabs, 0)), limits(1, 1));
    }

    #[test]
    fn stops_resizing_at_the_limit() {
        let layout = GridLayout::default();
        let placement = Placement {
            column_span: 6,
            ..Default::default()
        };
        let span = |delta: i64, c: SpanConstraints| {
            resize_placement(
                &placement,
                GridDelta {
                    columns: delta,
                    rows: 0,
                },
                &layout,
                c,
            )
            .column_span
        };
        let list = control_constraints(ControlKind::RelatedList, 12);
        assert_eq!(span(-5, list), 6);
        assert_eq!(span(20, list), 12);
        assert_eq!(span(-10, control_constraints(ControlKind::Boolean, 12)), 1);
    }
}
