//! Form design schema (PRD §13, §14): forms, controls, grid layout, and navigation.
//! Expressions are stored as source text; only the TypeScript side parses them.
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod checks;
mod grid;
pub mod upgrade;
pub use checks::{table_issues, validate, validate_tables};
/// Grid rules shared with dashboards (PRD §13: one grid system for forms and dashboards).
pub(crate) use checks::{
    validate_layout as validate_grid_layout, validate_span as validate_grid_span,
};
pub use grid::{Breakpoint, GridAlign, GridLayout, GridTrack, NamedRegion, Placement, TrackKind};

pub const DESIGN_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", try_from = "Value")]
pub struct DesignSchema {
    pub version: u32,
    pub forms: Vec<Form>,
    pub navigation: Vec<NavigationItem>,
    pub start_page: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentDesign {
    version: u32,
    #[serde(default)]
    forms: Vec<Form>,
    #[serde(default)]
    navigation: Vec<NavigationItem>,
    #[serde(default)]
    start_page: Option<String>,
}

impl TryFrom<Value> for DesignSchema {
    type Error = String;
    fn try_from(value: Value) -> Result<Self, String> {
        let current: CurrentDesign =
            serde_json::from_value(upgrade::upgrade_value(value)).map_err(|e| e.to_string())?;
        Ok(Self {
            version: current.version,
            forms: current.forms,
            navigation: current.navigation,
            start_page: current.start_page,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FormMode {
    #[default]
    List,
    Detail,
    Create,
    Edit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    #[default]
    Table,
    Query,
}

/// Where a form reads its records: a table (editable) or a saved query (read-only).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormSource {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    /// Query sources: `$name` to a TypeScript expression over `app` and `params`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<String, String>,
}

/// A form-level validation rule: `expression` must be true for the record to save.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormRule {
    pub id: String,
    pub expression: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Form {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: Option<FormSource>,
    #[serde(default = "default_modes")]
    pub modes: Vec<FormMode>,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default)]
    pub layout: GridLayout,
    /// Columns shown in list mode; empty shows every bound control.
    #[serde(default)]
    pub list_columns: Vec<String>,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    /// Form opened when a list row is clicked; none opens this form in detail mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_form_id: Option<String>,
    #[serde(default)]
    pub rules: Vec<FormRule>,
    /// List mode row filter expression (evaluated in TypeScript, `src/runtime/conditions.ts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Actions run on form events (PRD §17.4, `docs/decisions/form-events.md`).
    #[serde(default, skip_serializing_if = "FormEvents::is_empty")]
    pub events: FormEvents,
}

/// Form event → action id. The runtime (`src/runtime/formEvents.ts`) runs them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FormEvents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_load: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_current: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_update: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_update: Option<String>,
}

impl FormEvents {
    pub fn is_empty(&self) -> bool {
        self.bound().next().is_none()
    }
    /// Bound events as (label, action id), in firing order.
    pub fn bound(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("on load", &self.on_load),
            ("on current", &self.on_current),
            ("before update", &self.before_update),
            ("after update", &self.after_update),
        ]
        .into_iter()
        .filter_map(|(label, id)| Some((label, id.as_deref()?)))
    }
}
pub fn default_modes() -> Vec<FormMode> {
    vec![
        FormMode::List,
        FormMode::Detail,
        FormMode::Create,
        FormMode::Edit,
    ]
}
fn default_page_size() -> u32 {
    25
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ControlKind {
    Label,
    #[default]
    Text,
    Multiline,
    Number,
    Decimal,
    #[serde(alias = "checkbox")]
    Boolean,
    Date,
    Time,
    Datetime,
    Select,
    Relationship,
    Computed,
    Button,
    Section,
    Tabs,
    RelatedList,
    Image,
}
impl ControlKind {
    pub fn is_container(self) -> bool {
        matches!(self, Self::Section | Self::Tabs)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: String,
    pub kind: ControlKind,
    pub label: String,
    #[serde(default)]
    pub binding: Option<Binding>,
    #[serde(default)]
    pub validation: Validation,
    #[serde(default)]
    pub placement: Placement,
    /// Container (section or tabs control, plus tab page id) this control sits in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ControlParent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub computed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Static text for label controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SelectOption>,
    /// Saved query whose first column is the value and second (optional) the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_query_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Relationship>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabPage>,
    /// Inner grid of a section or tabs container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<GridLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related: Option<RelatedList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// Presentation variant, e.g. "toggle" for booleans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Conditional styles; the first rule whose `when` holds sets the tone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<ConditionalStyle>,
}

/// Named tone of a conditional style. The renderer maps it to a class, never to raw CSS.
/// A tone this build does not know (hand-edited YAML, a newer app) loads as `Other` and
/// saves back unchanged; the renderer ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Tone {
    Positive,
    Negative,
    Warning,
    Muted,
    #[default]
    Emphasis,
    Other(String),
}

impl Tone {
    pub fn as_str(&self) -> &str {
        match self {
            Tone::Positive => "positive",
            Tone::Negative => "negative",
            Tone::Warning => "warning",
            Tone::Muted => "muted",
            Tone::Emphasis => "emphasis",
            Tone::Other(name) => name,
        }
    }
}

impl From<String> for Tone {
    fn from(name: String) -> Self {
        match name.as_str() {
            "positive" => Tone::Positive,
            "negative" => Tone::Negative,
            "warning" => Tone::Warning,
            "muted" => Tone::Muted,
            "emphasis" => Tone::Emphasis,
            _ => Tone::Other(name),
        }
    }
}

impl Serialize for Tone {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Tone {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Tone::from)
    }
}

/// One conditional style rule: `tone` applies when the `when` expression is true.
/// Shared by form controls and dashboard table columns (`column` names the column there).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalStyle {
    pub id: String,
    #[serde(default)]
    pub when: String,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlParent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectOption {
    pub value: String,
    #[serde(default)]
    pub label: String,
}
/// One column of a multi-column key: `column` on this side, `target` on the other table.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyPair {
    pub column: String,
    pub target: String,
}
/// Foreign-key lookup: stores `value_column` of `table`, shows `display_column`.
/// A multi-column key lists every pair in `keys` (bound column first); choosing a
/// record writes all of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub table: String,
    pub value_column: String,
    pub display_column: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeyPair>,
    /// Row filter over the choices (`record` is a choice row, `parent` the edited record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabPage {
    pub id: String,
    pub label: String,
}
/// Child rows of `table` whose `foreign_key` equals the parent record's `parent_column`.
/// A multi-column key lists every pair in `keys` (`column` on the child, `target` on
/// the parent); rows match on all of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelatedList {
    pub table: String,
    pub foreign_key: String,
    pub parent_column: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeyPair>,
    #[serde(default)]
    pub columns: Vec<String>,
    /// Form used to add and edit child rows (embedded; it may not hold related lists).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_id: Option<String>,
    /// Row filter over the child rows (`record` is a child row, `parent` the parent record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// Legacy (v2) field; v3 binds to the form source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    pub column: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum NavKind {
    #[default]
    Form,
    Report,
    Dashboard,
    Table,
    Group,
}

/// Navigation entry; `group` items nest other items (nested navigation, not subforms).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NavigationItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: NavKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<FormMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavigationItem>,
}

impl Default for DesignSchema {
    /// A new document's design: one empty form and its navigation item, with UUIDv7 ids.
    fn default() -> Self {
        let form_id = uuid::Uuid::now_v7().to_string();
        let nav_id = uuid::Uuid::now_v7().to_string();
        Self {
            version: DESIGN_SCHEMA_VERSION,
            forms: vec![Form {
                id: form_id.clone(),
                name: "Main form".into(),
                modes: default_modes(),
                page_size: default_page_size(),
                ..Default::default()
            }],
            navigation: vec![NavigationItem {
                id: nav_id.clone(),
                label: "Main form".into(),
                kind: NavKind::Form,
                target_id: Some(form_id),
                ..Default::default()
            }],
            start_page: Some(nav_id),
        }
    }
}

/// Design issues including table and column references checked against the live schema.
#[tauri::command]
pub fn validate_design(
    window_label: String,
) -> Result<Vec<crate::archive::Issue>, crate::manager::AppError> {
    checks::validate_document_design(&window_label)
}

impl DesignSchema {
    /// Structural checks that make a design unloadable; dependency problems are [`validate`] issues.
    pub fn validate(&self) -> Result<(), String> {
        checks::structural(self)
    }
}

#[cfg(test)]
mod tests;
