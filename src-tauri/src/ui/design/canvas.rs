//! The design-time canvas (port of `DesignCanvas.tsx`): the form grid and the nested grids of
//! sections and tab pages, all editable on the shared `GridCanvas`.
//!
//! Test ids: the form canvas is `canvas` (labelled "<form name> layout"); a section or tab
//! page canvas is `canvas-<container id>` or `canvas-<tabs id>-<tab id>`. Every control is an
//! item whose id is the control's id (labelled with its label; see `ui::grid::canvas` for the
//! resize buttons and grip). A tab group shows its pages as `tab-page:<tabs id>:<tab id>`
//! buttons inside the group `tab-pages:<tabs id>` ("<label> tabs").

use super::studio::DesignStudio;
use crate::design::{Control, ControlKind, Form};
use crate::forms::{constraints::control_constraints, schema::control_kind_label};
use crate::ui::{
    grid::canvas::{GridCanvas, GridItem},
    prelude::*,
};

fn summary(control: &Control) -> String {
    let column = control.binding.as_ref().map(|b| b.column.as_str());
    match control.kind {
        ControlKind::Label => control.text.clone().unwrap_or_default(),
        ControlKind::Relationship if control.relationship.is_some() => {
            let r = control.relationship.as_ref().expect("checked");
            format!("{} → {}.{}", column.unwrap_or("?"), r.table, r.display_column)
        }
        ControlKind::RelatedList if control.related.is_some() => {
            let r = control.related.as_ref().expect("checked");
            format!("{} by {}", r.table, r.foreign_key)
        }
        ControlKind::Computed => match control.computed.as_deref() {
            Some(expression) if !expression.is_empty() => format!("= {expression}"),
            _ => "No expression".to_string(),
        },
        ControlKind::Button => if control.action_id.is_some() { "Runs an action" } else { "No action" }.to_string(),
        _ => column.unwrap_or("Unbound").to_string(),
    }
}

impl DesignStudio {
    /// The grid of the form (`parent` None) or of one container, limited to a tab page.
    pub(super) fn render_canvas(
        &mut self,
        form: &Form,
        parent: Option<&Control>,
        tab: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = parent.and_then(|p| p.layout.clone()).unwrap_or_else(|| form.layout.clone());
        let columns = layout.columns.len();
        let controls: Vec<Control> = form
            .controls
            .iter()
            .filter(|c| {
                c.parent.as_ref().map(|p| p.id.as_str()) == parent.map(|p| p.id.as_str())
                    && tab.is_none_or(|t| c.parent.as_ref().and_then(|p| p.tab.as_deref()) == Some(t))
            })
            .cloned()
            .collect();
        let (id, label) = match (parent, tab) {
            (None, _) => ("canvas".to_string(), format!("{} layout", form.name)),
            (Some(p), None) => (format!("canvas-{}", p.id), format!("{} layout", p.label)),
            (Some(p), Some(t)) => (format!("canvas-{}-{t}", p.id), format!("{} layout", p.label)),
        };
        let entity = cx.entity();
        let place = {
            let entity = entity.clone();
            move |id: &str, placement, _: &mut Window, cx: &mut App| {
                let id = id.to_string();
                entity.update(cx, |this, cx| this.place(&id, placement, cx));
            }
        };
        let mut canvas = GridCanvas::new(SharedString::from(id), layout)
            .editable(true)
            .selected(self.selected.clone().map(SharedString::from))
            .label(label)
            .on_select({
                let entity = entity.clone();
                move |id, _, cx| {
                    let id = id.to_string();
                    entity.update(cx, |this, cx| this.select(Some(id), cx));
                }
            })
            .on_resize(place.clone())
            .on_move(place);
        for control in controls {
            let child = self.render_card(form, &control, window, cx);
            canvas = canvas.item(
                GridItem::new(control.id.clone(), control.placement.clone(), child)
                    .label(control.label.clone())
                    .constraints(control_constraints(control.kind, columns)),
            );
        }
        canvas.into_any_element()
    }

    fn render_card(&mut self, form: &Form, control: &Control, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let conditional = control.visible_when.is_some() || control.enabled_when.is_some();
        let kind = format!(
            "{}{}{}",
            control_kind_label(control.kind),
            if control.validation.required { " · required" } else { "" },
            if conditional { " · conditional" } else { "" },
        );
        // Title and type tag on separate rows; both truncate with an ellipsis instead of wrapping.
        let header = v_flex()
            .min_w_0()
            .child(div().min_w_0().truncate().font_weight(FontWeight::BOLD).child(control.label.clone()))
            .child(div().min_w_0().truncate().ty(Type::Caption).text_color(muted).child(kind));
        let card = v_flex().min_w_0().overflow_hidden().gap_1().p_2().rounded_md().border_1().border_color(border).child(header);
        let stop = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();
        match control.kind {
            ControlKind::Section => {
                let inner = self.render_canvas(form, Some(control), None, window, cx);
                card.child(div().on_mouse_down(MouseButton::Left, stop).child(inner)).into_any_element()
            }
            ControlKind::Tabs => {
                let active = self
                    .active_tabs
                    .get(&control.id)
                    .cloned()
                    .or_else(|| control.tabs.first().map(|t| t.id.clone()));
                let entity = cx.entity();
                let strip = h_flex()
                    .id(SharedString::from(format!("tab-pages:{}", control.id)))
                    .aria_label(format!("{} tabs", control.label))
                    .gap_1()
                    .on_mouse_down(MouseButton::Left, stop)
                    .children(control.tabs.iter().map(|tab| {
                        let (tabs_id, tab_id) = (control.id.clone(), tab.id.clone());
                        let entity = entity.clone();
                        Button::new(SharedString::from(format!("tab-page:{}:{}", control.id, tab.id)))
                            .ghost()
                            .small()
                            .label(tab.label.clone())
                            .selected(active.as_deref() == Some(tab.id.as_str()))
                            .on_click(move |_, _, cx| {
                                let (tabs_id, tab_id) = (tabs_id.clone(), tab_id.clone());
                                entity.update(cx, |this, cx| {
                                    this.active_tabs.insert(tabs_id, tab_id);
                                    cx.notify();
                                });
                            })
                    }))
                    .test_support();
                let body = active.map(|tab| {
                    let inner = self.render_canvas(form, Some(control), Some(&tab), window, cx);
                    div().on_mouse_down(MouseButton::Left, stop).child(inner)
                });
                card.child(strip).children(body).into_any_element()
            }
            _ => card.child(div().min_w_0().truncate().ty(Type::Caption).text_color(muted).child(summary(control))).into_any_element(),
        }
    }
}
