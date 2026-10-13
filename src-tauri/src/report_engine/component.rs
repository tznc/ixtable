//! Typed view of a report definition for the engine. `crate::reports` stores component
//! settings verbatim (`Component::extra`); layout converts them once per run.

use crate::reports::{self, PageSetup};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::rc::Rc;

/// `Some(None)` for an explicit null, `None` when the field is absent.
fn double_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<f64>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

/// Presentation settings: gray levels only (0 = black, 1 = white).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Style {
    pub font_size: Option<f64>,
    pub bold: Option<bool>,
    pub align: Option<String>,
    pub border_width: Option<f64>,
    pub fill: Option<f64>,
    pub gray: Option<f64>,
}

/// The style a conditional formatting rule applies; absent fields keep the base style.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConditionStyle {
    pub bold: Option<bool>,
    pub gray: Option<f64>,
    #[serde(deserialize_with = "double_option")]
    pub fill: Option<Option<f64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Condition {
    pub id: String,
    pub when: String,
    pub style: ConditionStyle,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableColumn {
    pub id: String,
    pub header: String,
    pub expression: String,
    pub format: Option<String>,
    pub width: f64,
    pub align: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct SubreportLink {
    pub child: String,
    pub master: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartSpec {
    pub chart_type: String,
    pub query_id: Option<String>,
    pub x_field: String,
    pub y_fields: Vec<String>,
    pub group_by: Option<String>,
    pub stacked: bool,
    pub format: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    StaticText { text: String },
    Field { expression: String, format: Option<String> },
    Calculated { expression: String, format: Option<String> },
    Image { asset_id: String },
    Line { vertical: bool },
    Rectangle,
    Table { query_id: Option<String>, columns: Vec<TableColumn> },
    Subreport { report_id: String, links: Vec<SubreportLink> },
    Chart(ChartSpec),
}

/// A positioned component.
#[derive(Debug, Clone, PartialEq)]
pub struct Comp {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub style: Style,
    pub can_grow: bool,
    pub conditions: Vec<Condition>,
    /// "group" or "all".
    pub running_sum: Option<String>,
    pub kind: Kind,
}

impl Comp {
    /// A static text, field or calculated component.
    pub fn is_text(&self) -> bool {
        matches!(
            self.kind,
            Kind::StaticText { .. } | Kind::Field { .. } | Kind::Calculated { .. }
        )
    }
    pub fn is_table(&self) -> bool {
        matches!(self.kind, Kind::Table { .. })
    }
    pub fn is_subreport(&self) -> bool {
        matches!(self.kind, Kind::Subreport { .. })
    }
    /// Field or calculated component with a running sum.
    pub fn is_running(&self) -> bool {
        self.running_sum.is_some()
            && matches!(self.kind, Kind::Field { .. } | Kind::Calculated { .. })
    }
    pub fn expression(&self) -> &str {
        match &self.kind {
            Kind::Field { expression, .. } | Kind::Calculated { expression, .. } => expression,
            _ => "",
        }
    }
    pub fn format(&self) -> Option<&str> {
        match &self.kind {
            Kind::Field { format, .. } | Kind::Calculated { format, .. } => format.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Raw {
    id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    style: Style,
    can_grow: bool,
    conditions: Vec<Condition>,
    running_sum: Option<String>,
    text: String,
    expression: String,
    format: Option<String>,
    asset_id: String,
    orientation: Option<String>,
    query_id: Option<String>,
    columns: Vec<TableColumn>,
    report_id: String,
    links: Vec<SubreportLink>,
    chart_type: String,
    x_field: String,
    y_fields: Vec<String>,
    group_by: Option<String>,
    stacked: bool,
    title: Option<String>,
}

impl Comp {
    /// The typed component of a stored one; `None` for an unsupported kind.
    pub fn from_stored(c: &reports::Component) -> Option<Comp> {
        let mut map: Map<String, Value> = c.extra.clone();
        for (key, value) in [("id", Value::from(c.id.clone())), ("x", c.x.into()), ("y", c.y.into()), ("w", c.w.into()), ("h", c.h.into())] {
            map.insert(key.into(), value);
        }
        let raw: Raw = serde_json::from_value(Value::Object(map)).ok()?;
        let non_empty = |s: Option<String>| s.filter(|s| !s.is_empty());
        let kind = match c.kind.as_str() {
            "staticText" => Kind::StaticText { text: raw.text },
            "field" => Kind::Field { expression: raw.expression, format: raw.format },
            "calculated" => Kind::Calculated { expression: raw.expression, format: raw.format },
            "image" => Kind::Image { asset_id: raw.asset_id },
            "line" => Kind::Line { vertical: raw.orientation.as_deref() == Some("vertical") },
            "rectangle" => Kind::Rectangle,
            "table" => Kind::Table { query_id: non_empty(raw.query_id), columns: raw.columns },
            "subreport" => Kind::Subreport { report_id: raw.report_id, links: raw.links },
            "chart" => Kind::Chart(ChartSpec {
                chart_type: raw.chart_type,
                query_id: non_empty(raw.query_id),
                x_field: raw.x_field,
                y_fields: raw.y_fields,
                group_by: non_empty(raw.group_by),
                stacked: raw.stacked,
                format: raw.format,
                title: raw.title,
            }),
            _ => return None,
        };
        Some(Comp {
            id: raw.id,
            x: raw.x,
            y: raw.y,
            w: raw.w,
            h: raw.h,
            style: raw.style,
            can_grow: raw.can_grow,
            conditions: raw.conditions,
            running_sum: raw.running_sum,
            kind,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Band {
    pub height: f64,
    pub keep_together: bool,
    pub page_break_before: bool,
    pub page_break_after: bool,
    pub components: Vec<Comp>,
}

impl Band {
    pub fn from_stored(b: &reports::Band) -> Band {
        Band {
            height: b.height,
            keep_together: b.keep_together,
            page_break_before: b.page_break_before,
            page_break_after: b.page_break_after,
            components: b.components.iter().filter_map(Comp::from_stored).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: String,
    pub group_by: String,
    pub descending: bool,
    pub header: Rc<Band>,
    pub footer: Rc<Band>,
    pub new_page: bool,
    pub repeat_header: bool,
    pub reset_page_number: bool,
}

#[derive(Debug, Clone)]
pub struct Bands {
    pub report_header: Rc<Band>,
    pub page_header: Rc<Band>,
    pub groups: Vec<Group>,
    pub detail: Rc<Band>,
    pub page_footer: Rc<Band>,
    pub report_footer: Rc<Band>,
}

/// A report definition ready for layout.
#[derive(Debug, Clone)]
pub struct Rep {
    pub id: String,
    pub name: String,
    pub params: Map<String, Value>,
    pub page: PageSetup,
    pub bands: Bands,
}

impl Rep {
    pub fn from_stored(r: &reports::Report) -> Rep {
        let band = |b: &reports::Band| Rc::new(Band::from_stored(b));
        let b = &r.bands;
        Rep {
            id: r.id.clone(),
            name: r.name.clone(),
            params: r.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            page: r.page.clone(),
            bands: Bands {
                report_header: band(&b.report_header),
                page_header: band(&b.page_header),
                groups: b
                    .groups
                    .iter()
                    .map(|g| Group {
                        id: g.id.clone(),
                        group_by: g.group_by.clone(),
                        descending: g.descending,
                        header: band(&g.header),
                        footer: band(&g.footer),
                        new_page: g.new_page,
                        repeat_header: g.repeat_header,
                        reset_page_number: g.reset_page_number,
                    })
                    .collect(),
                detail: band(&b.detail),
                page_footer: band(&b.page_footer),
                report_footer: band(&b.report_footer),
            },
        }
    }
}
