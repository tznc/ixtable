//! The control tree of a form (port of `src/runtime/FormBody.tsx` and the static controls of
//! `controls.tsx`): labels, sections, tab groups, computed values, buttons, images, related
//! lists and bound inputs, laid out on the form's grid.
//!
//! Test ids (`<prefix>` is empty except in repeated rows): `<prefix>grid:<container>` is a
//! grid canvas, `<prefix>label:<slug>`, `<prefix>section:<slug>`, `<prefix>computed:<slug>`,
//! `<prefix>button:<slug>`, `<prefix>image:<slug>` (labelled by the control; its `:state` child
//! reads "Loading image…", "No image", "Image unavailable" or "Image"), and for a tab group
//! `<prefix>tabs:<slug>` with tab buttons `<prefix>tab:<tab label slug>` (selected = shown,
//! the label gains " (has errors)" when a control inside has an error).

use super::{BodyKey, RuntimeEnv};
use super::field::{FieldProps, FieldView};
use crate::design::{Control, ControlKind, Form};
use crate::forms::conditions::tone_for;
use crate::forms::constraints::control_constraints;
use crate::forms::schema::{form_table, is_input_kind};
use crate::forms::state::{FormScope, compute, formatted};
use crate::ui::grid::canvas::{GridCanvas, GridItem};
use crate::ui::prelude::*;
use gpui_kit::component::h_flex;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What the control tree needs from the view that owns the record.
#[derive(Clone)]
pub struct BodyContext {
    pub env: RuntimeEnv,
    pub form: Rc<Form>,
    pub scope: FormScope,
    pub visible: Rc<HashSet<String>>,
    /// Controls whose own and every ancestor container's `enabledWhen` holds.
    pub enabled: Rc<HashSet<String>>,
    /// Validation message by control id.
    pub errors: Rc<HashMap<String, String>>,
    /// True in detail mode or when the form is read-only.
    pub read_only: bool,
    /// Columns that may not be edited (the related-list link, auto keys).
    pub locked: Rc<HashSet<String>>,
    /// True on a continuous form's repeated rows, which hold no related lists.
    pub repeated: bool,
    /// Prefix of every element id, distinguishing repeated rows.
    pub prefix: BodyKey,
    /// Button controls that may run now.
    pub runnable: Rc<HashSet<String>>,
    pub on_set: Rc<dyn Fn(&str, Value, &mut App)>,
    pub on_blur: Rc<dyn Fn(&str, &mut App)>,
    pub on_button: Rc<dyn Fn(&str, &mut Window, &mut App)>,
    pub on_enter: Rc<dyn Fn(&mut App)>,
    /// Builds a related-record list for a control.
    pub related: Rc<dyn Fn(&Control, bool, &mut Window, &mut App) -> AnyElement>,
}

fn children_of<'a>(form: &'a Form, parent: Option<&str>, tab: Option<&str>) -> Vec<&'a Control> {
    form.controls
        .iter()
        .filter(|c| c.parent.as_ref().map(|p| p.id.as_str()) == parent)
        .filter(|c| tab.is_none_or(|t| c.parent.as_ref().and_then(|p| p.tab.as_deref()) == Some(t)))
        .collect()
}

/// Renders the controls of one container (the form itself when `parent` is None) on the grid.
pub fn control_grid(
    ctx: &BodyContext,
    parent: Option<&Control>,
    tab: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let layout = parent
        .and_then(|p| p.layout.clone())
        .unwrap_or_else(|| ctx.form.layout.clone());
    let columns = layout.columns.len();
    let name = format!(
        "{}grid:{}{}",
        ctx.prefix,
        parent.map_or("form", |p| p.id.as_str()),
        tab.map(|t| format!(":{t}")).unwrap_or_default()
    );
    let mut canvas = GridCanvas::new(name, layout);
    for control in children_of(&ctx.form, parent.map(|p| p.id.as_str()), tab) {
        if !ctx.visible.contains(&control.id) {
            continue;
        }
        let view = control_view(ctx, control, window, cx);
        canvas = canvas.item(
            GridItem::new(
                SharedString::from(format!("{}item:{}", ctx.prefix, control.id)),
                control.placement.clone(),
                view,
            )
            .label(control.label.clone())
            .constraints(control_constraints(control.kind, columns)),
        );
    }
    canvas.into_any_element()
}

/// Column headers for repeated rows: the form's grid with each input control's label in its cell.
pub fn header_grid(form: &Form) -> AnyElement {
    let layout = form.layout.clone();
    let columns = layout.columns.len();
    let mut canvas = GridCanvas::new("header-grid", layout);
    for control in children_of(form, None, None) {
        if !is_input_kind(control.kind) {
            continue;
        }
        let text = if control.validation.required { format!("{} *", control.label) } else { control.label.clone() };
        let view = widgets::styled_text(
            SharedString::from(format!("header:{}", control.id)),
            text,
            |d| d.text_sm().font_weight(FontWeight::MEDIUM),
        );
        canvas = canvas.item(
            GridItem::new(
                SharedString::from(format!("header-item:{}", control.id)),
                control.placement.clone(),
                view.into_any_element(),
            )
            .label(control.label.clone())
            .constraints(control_constraints(control.kind, columns)),
        );
    }
    canvas.into_any_element()
}

fn named(prefix: &str, kind: &str, control: &Control) -> SharedString {
    let slug = widgets::slug(&control.label);
    format!("{prefix}{kind}:{}", if slug.is_empty() { control.id.clone() } else { slug }).into()
}

fn control_view(ctx: &BodyContext, control: &Control, window: &mut Window, cx: &mut App) -> AnyElement {
    let scope = ctx.scope.scope();
    let column = control.binding.as_ref().map(|b| b.column.clone()).filter(|c| !c.is_empty());
    match control.kind {
        ControlKind::Label => {
            let text = control.text.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| control.label.clone());
            return widgets::styled_text(named(&ctx.prefix, "label", control), text, |d| d.text_sm())
                .into_any_element();
        }
        ControlKind::Section => {
            return v_flex()
                .id(named(&ctx.prefix, "section", control))
                .aria_label(control.label.clone())
                .gap_2()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(div().font_weight(FontWeight::SEMIBOLD).text_sm().child(control.label.clone()))
                .child(control_grid(ctx, Some(control), None, window, cx))
                .test_support()
                .into_any_element();
        }
        ControlKind::Tabs => return tab_group(ctx, control, window, cx),
        ControlKind::Computed => {
            let result = compute(control.computed.as_deref(), scope);
            let mut with_value = scope.clone();
            with_value.insert("value".into(), result.value.clone());
            let tone = tone_for(&control.styles, &with_value, column.as_deref());
            return computed_value(ctx, control, &result.value, result.error.as_deref(), tone, cx);
        }
        ControlKind::Button => {
            let enabled = ctx.enabled.contains(&control.id) && ctx.runnable.contains(&control.id);
            let on_button = ctx.on_button.clone();
            let id = control.id.clone();
            return Button::new(named(&ctx.prefix, "button", control))
                .label(control.label.clone())
                .disabled(!enabled)
                .on_click(move |_, window, cx| on_button(&id, window, cx))
                .into_any_element();
        }
        ControlKind::Image => return image_view(ctx, control, window, cx),
        ControlKind::RelatedList => {
            if ctx.repeated {
                return div().into_any_element();
            }
            return (ctx.related)(control, !ctx.enabled.contains(&control.id), window, cx);
        }
        _ => {}
    }
    if !is_input_kind(control.kind) {
        return div().into_any_element();
    }
    let derived = control.computed.as_deref().filter(|c| !c.trim().is_empty()).map(|c| compute(Some(c), scope));
    let value = match (&derived, &column) {
        (Some(d), _) => d.value.clone(),
        (None, Some(column)) => ctx.scope.record().get(column).cloned().unwrap_or(Value::Null),
        (None, None) => Value::Null,
    };
    let read_only = ctx.read_only
        || derived.is_some()
        || control.read_only
        || column.as_ref().is_none_or(|c| ctx.locked.contains(c))
        || !ctx.enabled.contains(&control.id);
    let mut with_value = scope.clone();
    with_value.insert("value".into(), value.clone());
    let tone = tone_for(&control.styles, &with_value, column.as_deref());
    if ctx.read_only
        && control.format.as_deref().is_some_and(|f| !f.is_empty())
        && !value.is_null()
        && control.kind != ControlKind::Relationship
    {
        return computed_value(ctx, control, &value, None, tone, cx);
    }
    let filter_scope = control.relationship.as_ref().and_then(|r| r.filter.as_ref()).map(|_| {
        let mut scope = Map::new();
        scope.insert("parent".into(), Value::Object(ctx.scope.record().clone()));
        for name in ["form", "app"] {
            scope.insert(name.into(), scope_value(ctx.scope.scope(), name));
        }
        scope.insert("params".into(), Value::Object(Map::new()));
        scope
    });
    let field = window.use_keyed_state(
        (ElementId::from(SharedString::from(format!("{}fv:{}", ctx.prefix, control.id))), "field"),
        cx,
        |_, _| FieldView::new(),
    );
    let (on_set, on_blur, on_enter) = (ctx.on_set.clone(), ctx.on_blur.clone(), ctx.on_enter.clone());
    let (set_for_keys, control_id) = (ctx.on_set.clone(), control.id.clone());
    let set_column = column.clone();
    let props = FieldProps {
        label: control.label.clone().into(),
        hide_label: ctx.repeated,
        control: control.clone(),
        value,
        read_only,
        error: ctx.errors.get(&control.id).cloned(),
        tone,
        filter_scope,
        key_values: ctx.scope.record().clone(),
        table: form_table(&ctx.form).map(str::to_string),
        prefix: ctx.prefix.clone(),
        window_label: ctx.env.label.clone(),
        on_change: Rc::new(move |value, cx| {
            if let Some(column) = &set_column {
                on_set(column, value, cx);
            }
        }),
        on_keys: Rc::new(move |values, cx| {
            for (key, value) in values {
                set_for_keys(&key, value, cx);
            }
        }),
        on_blur: Rc::new(move |cx| on_blur(&control_id, cx)),
        on_enter,
    };
    field.update(cx, |f, cx| f.set_props(props, window, cx));
    field.into_any_element()
}

fn scope_value(scope: &Map<String, Value>, name: &str) -> Value {
    scope.get(name).cloned().unwrap_or_else(|| Value::Object(Map::new()))
}

/// Read-only computed value with the control's format.
pub fn computed_value(
    ctx: &BodyContext,
    control: &Control,
    value: &Value,
    error: Option<&str>,
    tone: Option<crate::design::Tone>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let color = tone.as_ref().and_then(|t| widgets::tone_color(t, cx));
    let shown = if error.is_some() { "—".to_string() } else { formatted(value, control.format.as_deref()) };
    v_flex()
        .gap_1()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).text_color(theme.muted_foreground).child(control.label.clone()))
        .child(widgets::styled_text(named(&ctx.prefix, "computed", control), shown, move |d| {
            d.text_sm().when_some(color, |d, c| d.text_color(c))
        }))
        .into_any_element()
}

// ---- tabs -----------------------------------------------------------------------------

struct TabsState {
    active: String,
}

fn tab_group(ctx: &BodyContext, control: &Control, window: &mut Window, cx: &mut App) -> AnyElement {
    let state = window.use_keyed_state(
        (ElementId::from(SharedString::from(format!("{}tabs:{}", ctx.prefix, control.id))), "tabs"),
        cx,
        |_, _| TabsState { active: String::new() },
    );
    let tabs = control.tabs.clone();
    let active = {
        let current = state.read(cx).active.clone();
        tabs.iter().find(|t| t.id == current).or(tabs.first()).map(|t| t.id.clone()).unwrap_or_default()
    };
    let error_tabs: HashSet<String> = ctx
        .form
        .controls
        .iter()
        .filter(|c| c.parent.as_ref().is_some_and(|p| p.id == control.id) && ctx.errors.contains_key(&c.id))
        .filter_map(|c| c.parent.as_ref().and_then(|p| p.tab.clone()))
        .collect();
    let theme = cx.theme().clone();
    let prefix = ctx.prefix.clone();
    let order: Vec<String> = tabs.iter().map(|t| t.id.clone()).collect();
    let strip = h_flex()
        .id(SharedString::from(format!("{prefix}tabs:{}", widgets::slug(&control.label))))
        .aria_label(control.label.clone())
        .gap_1()
        .border_b_1()
        .border_color(theme.border)
        .on_key_down({
            let (state, active, order) = (state.clone(), active.clone(), order.clone());
            move |event, window, cx| {
                let step: i64 = match event.keystroke.key.as_str() {
                    "right" => 1,
                    "left" => -1,
                    _ => return,
                };
                let Some(index) = order.iter().position(|t| *t == active) else { return };
                let next = order[(index as i64 + step).rem_euclid(order.len() as i64) as usize].clone();
                cx.stop_propagation();
                state.update(cx, |s, cx| {
                    s.active = next;
                    cx.notify();
                });
                window.refresh();
            }
        })
        .children(tabs.iter().map(|tab| {
            let label = if error_tabs.contains(&tab.id) {
                format!("{} (has errors)", tab.label)
            } else {
                tab.label.clone()
            };
            let (state, id) = (state.clone(), tab.id.clone());
            widgets::tab_button(SharedString::from(format!("{prefix}tab:{}", widgets::slug(&tab.label))), label, tab.id == active, cx)
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    state.update(cx, |s, cx| {
                        s.active = id;
                        cx.notify();
                    });
                })
        }))
        .test_support();
    let panel = tabs
        .iter()
        .find(|t| t.id == active)
        .map(|tab| control_grid(ctx, Some(control), Some(&tab.id), window, cx));
    v_flex().gap_2().child(strip).children(panel).into_any_element()
}

// ---- images ---------------------------------------------------------------------------

/// An image control's state: the asset is read off the UI thread.
struct ImageState {
    asset: String,
    shown: Option<Result<Arc<Image>, String>>,
}

use std::sync::Arc;

fn image_view(ctx: &BodyContext, control: &Control, window: &mut Window, cx: &mut App) -> AnyElement {
    let asset = control.asset_id.clone().unwrap_or_default();
    let state = window.use_keyed_state(
        (ElementId::from(SharedString::from(format!("{}image:{}", ctx.prefix, control.id))), "image"),
        cx,
        |_, _| ImageState { asset: String::new(), shown: None },
    );
    if state.read(cx).asset != asset {
        state.update(cx, |s, _| {
            s.asset = asset.clone();
            s.shown = None;
        });
        if !asset.is_empty() {
            let label = ctx.env.label.to_string();
            let (state, id) = (state.clone(), asset.clone());
            let job = cx.background_spawn(async move { crate::asset_data::read_asset_data_url(label, id) });
            let asset = asset.clone();
            cx.spawn(async move |cx| {
                let result = job.await;
                let _ = state.update(cx, |s, cx| {
                    if s.asset != asset {
                        return;
                    }
                    s.shown = Some(match result {
                        Ok(url) => decode_data_url(&url),
                        Err(e) => Err(e.message),
                    });
                    cx.notify();
                });
            })
            .detach();
        }
    }
    let theme = cx.theme().clone();
    let (status, picture) = match (&state.read(cx).shown, asset.is_empty()) {
        (_, true) => ("No image".to_string(), None),
        (None, false) => ("Loading image…".to_string(), None),
        (Some(Err(_)), false) => ("Image unavailable".to_string(), None),
        (Some(Ok(image)), false) => ("Image".to_string(), Some(image.clone())),
    };
    v_flex()
        .id(named(&ctx.prefix, "image", control))
        .aria_label(control.label.clone())
        .gap_1()
        .child(match picture {
            Some(image) => img(image).max_h(px(240.)).into_any_element(),
            None => div()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(status.clone())
                .into_any_element(),
        })
        .child(widgets::styled_text(
            SharedString::from(format!("{}:state", named(&ctx.prefix, "image", control))),
            status,
            |d| d.text_xs(),
        ))
        .child(div().text_xs().text_color(theme.muted_foreground).child(control.label.clone()))
        .test_support()
        .into_any_element()
}

pub(crate) fn decode_data_url(url: &str) -> Result<Arc<Image>, String> {
    use base64::Engine as _;
    let (head, body) = url.split_once(',').ok_or("Not a data URL")?;
    let media = head.trim_start_matches("data:").split(';').next().unwrap_or("");
    let format = match media {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/svg+xml" => ImageFormat::Svg,
        "image/bmp" => ImageFormat::Bmp,
        other => return Err(format!("Unsupported image type {other}")),
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(body).map_err(|e| e.to_string())?;
    Ok(Arc::new(Image::from_bytes(format, bytes)))
}

/// One danger callout for a list of problems. A single problem is the headline; several
/// get a count headline and one line each (`<id>-body`). The wrapper is `<id>`.
pub fn problems_callout(id: &str, problems: &[String], cx: &App) -> AnyElement {
    let (title, body) = match problems {
        [one] => (one.clone(), None),
        many => (format!("{} to fix", crate::ui::widgets::plural(many.len(), "problem")), Some(many.join("\n"))),
    };
    v_flex()
        .id(SharedString::from(id.to_string()))
        .aria_label("Problems")
        .child(crate::ui::widgets::callout(
            SharedString::from(format!("{id}-text")),
            crate::ui::widgets::Severity::Danger,
            &title,
            body.as_deref(),
            cx,
        ))
        .test_support()
        .into_any_element()
}
