//! Related-list nesting limits (port of `src/design/nesting.ts`).

use crate::design::{ControlKind, Form};
use std::collections::HashSet;

/// Related lists nest up to three levels below the form that opens them (PRD Phase 7).
pub const MAX_SUBFORM_DEPTH: usize = 3;

/// A depth that is infinite when forms embed each other in a cycle.
pub type Depth = Option<usize>;

/// Forms a form's related lists open (only lists that name a form).
pub fn embedded_form_ids(form: &Form) -> Vec<String> {
    form.controls
        .iter()
        .filter(|c| c.kind == ControlKind::RelatedList)
        .filter_map(|c| c.related.as_ref().and_then(|r| r.form_id.clone()))
        .collect()
}

/// Levels of related lists below `form_id`: 0 when it embeds no form, 1 when its related
/// lists open forms without their own, and so on. A cycle is infinitely deep (`None`).
pub fn subform_depth(forms: &[Form], form_id: &str, path: &HashSet<String>) -> Depth {
    if path.contains(form_id) {
        return None;
    }
    let Some(form) = forms.iter().find(|f| f.id == form_id) else {
        return Some(0);
    };
    let ids = embedded_form_ids(form);
    if ids.is_empty() {
        return Some(0);
    }
    let mut inner = path.clone();
    inner.insert(form_id.to_string());
    let mut deepest = 0;
    for id in ids {
        deepest = deepest.max(subform_depth(forms, &id, &inner)?);
    }
    Some(1 + deepest)
}

/// Levels of related lists above `form_id`: the longest chain of forms that embed it.
pub fn subform_height(forms: &[Form], form_id: &str, path: &HashSet<String>) -> Depth {
    if path.contains(form_id) {
        return None;
    }
    let mut inner = path.clone();
    inner.insert(form_id.to_string());
    let mut tallest = 0;
    for parent in forms
        .iter()
        .filter(|f| embedded_form_ids(f).iter().any(|id| id == form_id))
    {
        tallest = tallest.max(1 + subform_height(forms, &parent.id, &inner)?);
    }
    Some(tallest)
}

/// Forms a related list on `form` may open: never `form` itself or a form that embeds it,
/// and only when the whole chain stays within [`MAX_SUBFORM_DEPTH`].
pub fn embeddable_forms<'a>(forms: &'a [Form], form: &Form) -> Vec<&'a Form> {
    let above = subform_height(forms, &form.id, &HashSet::new());
    forms
        .iter()
        .filter(|child| {
            if child.id == form.id {
                return false;
            }
            let below = subform_depth(forms, &child.id, &HashSet::from([form.id.clone()]));
            match (above, below) {
                (Some(a), Some(b)) => a + 1 + b <= MAX_SUBFORM_DEPTH,
                _ => false,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{MAX_SUBFORM_DEPTH, embeddable_forms, subform_depth, subform_height};
    use crate::design::{ControlKind, Form, RelatedList};
    use crate::forms::schema::{new_control, new_form};
    use std::collections::HashSet;

    fn form(id: &str, children: &[&str]) -> Form {
        let mut base = new_form(id, None);
        base.id = id.into();
        let controls = children
            .iter()
            .map(|child| {
                let mut control = new_control(ControlKind::RelatedList, &base, None);
                control.related = Some(RelatedList {
                    table: "t".into(),
                    foreign_key: "p".into(),
                    parent_column: "id".into(),
                    form_id: Some((*child).into()),
                    ..Default::default()
                });
                control
            })
            .collect();
        Form { controls, ..base }
    }

    fn depth(forms: &[Form], id: &str) -> Option<usize> {
        subform_depth(forms, id, &HashSet::new())
    }
    fn height(forms: &[Form], id: &str) -> Option<usize> {
        subform_height(forms, id, &HashSet::new())
    }

    #[test]
    fn measures_levels_below_and_above_a_form() {
        let forms = vec![
            form("a", &["b"]),
            form("b", &["c", "x"]),
            form("c", &["d"]),
            form("d", &[]),
            form("x", &[]),
        ];
        assert_eq!(MAX_SUBFORM_DEPTH, 3);
        assert_eq!(depth(&forms, "a"), Some(3));
        assert_eq!(depth(&forms, "d"), Some(0));
        assert_eq!(height(&forms, "d"), Some(3));
        assert_eq!(height(&forms, "a"), Some(0));
        let looped = vec![form("a", &["b"]), form("b", &["a"])];
        assert_eq!(depth(&looped, "a"), None);
        assert_eq!(height(&looped, "a"), None);
    }

    #[test]
    fn offers_child_forms_that_stay_within_three_levels_and_never_loop() {
        let forms = vec![
            form("a", &["b"]),
            form("b", &["c"]),
            form("c", &[]),
            form("d", &["e"]),
            form("e", &[]),
        ];
        let offered = |id: &str| -> Vec<String> {
            let f = forms.iter().find(|f| f.id == id).unwrap();
            embeddable_forms(&forms, f)
                .iter()
                .map(|f| f.id.clone())
                .collect()
        };
        assert_eq!(offered("a"), ["b", "c", "d", "e"]);
        assert_eq!(offered("c"), ["e"]);
        assert_eq!(offered("b"), ["c", "d", "e"]);
    }
}
