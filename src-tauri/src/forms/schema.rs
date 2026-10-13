//! Design helpers shared by the form designer, the generators and the Runtime
//! (port of the non-type parts of `src/design/schema.ts`). The data model itself is
//! `crate::design`.

use crate::design::{
    Control, ControlKind, ControlParent, DesignSchema, Form, FormMode, FormSource, GridLayout,
    KeyPair, NavKind, NavigationItem, Placement, RelatedList, SourceKind, TabPage, Validation,
};
use crate::ui::grid::engine::{self, default_grid_layout};

pub use crate::design::default_modes as default_form_modes;

pub const FORM_MODES: [FormMode; 6] = [
    FormMode::List,
    FormMode::Detail,
    FormMode::Create,
    FormMode::Edit,
    FormMode::Continuous,
    FormMode::Split,
];

/// Modes that show many records at once; the rest show one record.
pub fn is_collection_mode(mode: FormMode) -> bool {
    matches!(mode, FormMode::List | FormMode::Continuous | FormMode::Split)
}

pub fn form_mode_label(mode: FormMode) -> &'static str {
    match mode {
        FormMode::List => "List",
        FormMode::Detail => "Detail",
        FormMode::Create => "Create",
        FormMode::Edit => "Edit",
        FormMode::Continuous => "Continuous",
        FormMode::Split => "Split",
    }
}

/// The `camelCase` key of a mode, as stored in navigation items and shown in tests.
pub fn form_mode_key(mode: FormMode) -> &'static str {
    match mode {
        FormMode::List => "list",
        FormMode::Detail => "detail",
        FormMode::Create => "create",
        FormMode::Edit => "edit",
        FormMode::Continuous => "continuous",
        FormMode::Split => "split",
    }
}

pub fn form_mode_from_key(key: &str) -> Option<FormMode> {
    FORM_MODES.into_iter().find(|m| form_mode_key(*m) == key)
}

pub const CONTROL_KINDS: [ControlKind; 20] = [
    ControlKind::Label,
    ControlKind::Text,
    ControlKind::Multiline,
    ControlKind::Number,
    ControlKind::Decimal,
    ControlKind::Boolean,
    ControlKind::Date,
    ControlKind::Time,
    ControlKind::Datetime,
    ControlKind::Select,
    ControlKind::Relationship,
    ControlKind::Computed,
    ControlKind::Button,
    ControlKind::Section,
    ControlKind::Tabs,
    ControlKind::RelatedList,
    ControlKind::Image,
    ControlKind::RichText,
    ControlKind::Attachment,
    ControlKind::MultiSelect,
];

pub fn control_kind_label(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::Label => "Text",
        ControlKind::Text => "Text field",
        ControlKind::Multiline => "Notes",
        ControlKind::Number => "Number",
        ControlKind::Decimal => "Amount",
        ControlKind::Boolean => "Yes/No",
        ControlKind::Date => "Date",
        ControlKind::Time => "Time",
        ControlKind::Datetime => "Date and time",
        ControlKind::Select => "Choice",
        ControlKind::Relationship => "Lookup",
        ControlKind::Computed => "Computed value",
        ControlKind::Button => "Button",
        ControlKind::Section => "Section",
        ControlKind::Tabs => "Tabs",
        ControlKind::RelatedList => "Related records",
        ControlKind::Image => "Image",
        ControlKind::RichText => "Rich text",
        ControlKind::Attachment => "Attachments",
        ControlKind::MultiSelect => "Multiple choices",
    }
}

/// Static kinds have nothing to enable or disable, so `enabled_when` does not apply to them.
pub fn has_enabled_state(kind: ControlKind) -> bool {
    !matches!(kind, ControlKind::Label | ControlKind::Image)
}

/// Kinds that hold a record value bound to a column.
pub fn is_input_kind(kind: ControlKind) -> bool {
    matches!(
        kind,
        ControlKind::Text
            | ControlKind::Multiline
            | ControlKind::Number
            | ControlKind::Decimal
            | ControlKind::Boolean
            | ControlKind::Date
            | ControlKind::Time
            | ControlKind::Datetime
            | ControlKind::Select
            | ControlKind::Relationship
            | ControlKind::RichText
            | ControlKind::Attachment
            | ControlKind::MultiSelect
    )
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn same_parent(a: Option<&ControlParent>, b: Option<&ControlParent>) -> bool {
    a.map(|p| (&p.id, p.tab.as_deref())) == b.map(|p| (&p.id, p.tab.as_deref()))
}

/// The grid a control is placed on: its container's inner layout, else the form's.
pub fn container_layout<'a>(form: &'a Form, parent: Option<&ControlParent>) -> &'a GridLayout {
    parent
        .and_then(|p| form.controls.iter().find(|c| c.id == p.id))
        .and_then(|c| c.layout.as_ref())
        .unwrap_or(&form.layout)
}

/// First free slot in the given container (the form grid by default). The size defaults
/// to the full width and one row.
pub fn next_placement(
    form: &Form,
    parent: Option<&ControlParent>,
    column_span: Option<i64>,
    row_span: Option<i64>,
) -> Placement {
    let layout = if parent.is_some() {
        container_layout(form, parent)
    } else {
        &form.layout
    };
    let siblings: Vec<Placement> = form
        .controls
        .iter()
        .filter(|c| same_parent(c.parent.as_ref(), parent))
        .map(|c| c.placement.clone())
        .collect();
    engine::next_placement(layout, &siblings, column_span, row_span)
}

fn container_grid() -> GridLayout {
    GridLayout {
        column_gap: 12,
        row_gap: 12,
        ..default_grid_layout()
    }
}

pub fn new_control(kind: ControlKind, form: &Form, parent: Option<&ControlParent>) -> Control {
    let wide = kind.is_container() || kind == ControlKind::RelatedList;
    let mut control = Control {
        id: new_id(),
        kind,
        label: if kind == ControlKind::Section {
            "New section".into()
        } else {
            format!("New {}", control_kind_label(kind).to_lowercase())
        },
        validation: Validation::default(),
        placement: next_placement(form, parent, (!wide).then_some(6), None),
        parent: parent.cloned(),
        ..Default::default()
    };
    match kind {
        ControlKind::Label => control.text = Some("Text".into()),
        ControlKind::Computed => control.computed = Some(String::new()),
        ControlKind::Section => control.layout = Some(container_grid()),
        ControlKind::Tabs => {
            control.layout = Some(container_grid());
            control.tabs = vec![
                TabPage {
                    id: new_id(),
                    label: "Tab 1".into(),
                },
                TabPage {
                    id: new_id(),
                    label: "Tab 2".into(),
                },
            ];
        }
        _ => {}
    }
    control
}

pub fn new_form(name: &str, source: Option<FormSource>) -> Form {
    let query = source.as_ref().is_some_and(|s| s.kind == SourceKind::Query);
    Form {
        id: new_id(),
        name: name.into(),
        source,
        modes: if query {
            vec![FormMode::List, FormMode::Detail]
        } else {
            default_form_modes()
        },
        layout: default_grid_layout(),
        page_size: 25,
        navigation_bar: true,
        ..Default::default()
    }
}

/// Every navigation item, depth first.
pub fn flatten_navigation(items: &[NavigationItem]) -> Vec<&NavigationItem> {
    items
        .iter()
        .flat_map(|item| {
            std::iter::once(item).chain(flatten_navigation(&item.children))
        })
        .collect()
}

/// The table a form reads, when its source is a table.
pub fn form_table(form: &Form) -> Option<&str> {
    let source = form.source.as_ref()?;
    (source.kind == SourceKind::Table)
        .then_some(source.table.as_deref())
        .flatten()
}

/// Key pairs of a related list: `keys` when set, else `foreign_key` to `parent_column`.
pub fn related_keys(related: &RelatedList) -> Vec<KeyPair> {
    if related.keys.is_empty() {
        vec![KeyPair {
            column: related.foreign_key.clone(),
            target: related.parent_column.clone(),
        }]
    } else {
        related.keys.clone()
    }
}

/// A navigation entry that opens a form.
pub fn form_nav(id: String, label: &str, form_id: &str) -> NavigationItem {
    NavigationItem {
        id,
        label: label.into(),
        kind: NavKind::Form,
        target_id: Some(form_id.into()),
        ..Default::default()
    }
}

/// Brings any stored design (v1 to v3) to the current shape with defaults filled, through
/// the same upgrade the loader runs (`design::upgrade`).
pub fn upgrade_design(raw: serde_json::Value) -> Result<DesignSchema, String> {
    serde_json::from_value(raw).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
