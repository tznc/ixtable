//! `ActionPicker` of the React app: choose one of the document's actions. Forms, dashboards
//! and the trigger editor use it wherever an action can be attached.
//!
//! The control is a choice button with the id the caller gives it; its options are the
//! items (by index) of the `popup-menu` it opens: index 0 is `(none)`, then the actions in
//! definition order.

use super::fields::choice_field;
use crate::automation::ActionDef;
use gpui_kit::*;

/// A labeled action choice. `value` is an action id (`None` or empty means none); an id
/// that is not an action shows as `Missing action (id)`. `exclude` hides one action (the
/// one being edited). `on_pick` gets the chosen id, or `None` for `(none)`.
pub fn action_picker(
    id: impl Into<ElementId>,
    label: &str,
    value: Option<&str>,
    exclude: Option<&str>,
    actions: &[ActionDef],
    on_pick: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let current = value.unwrap_or("");
    let mut options: Vec<(String, String)> = vec![(String::new(), "(none)".to_string())];
    options.extend(
        actions
            .iter()
            .filter(|a| Some(a.id.as_str()) != exclude)
            .map(|a| {
                let name = if a.name.is_empty() { a.id.clone() } else { a.name.clone() };
                (a.id.clone(), name)
            }),
    );
    if !current.is_empty() && !actions.iter().any(|a| a.id == current) {
        options.push((current.to_string(), format!("Missing action ({current})")));
    }
    choice_field(
        id,
        label,
        current,
        options,
        "(none)",
        move |picked, window, cx| {
            on_pick((!picked.is_empty()).then(|| picked.to_string()), window, cx)
        },
        cx,
    )
}
