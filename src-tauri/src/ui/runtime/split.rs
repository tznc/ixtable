//! Split mode (port of `SplitView.tsx`): the list as a datasheet above the selected record's
//! detail view. Choosing a row shows it below instead of opening a new view; saving in the
//! detail pane re-reads the list. The detail form is `detailFormId`, else this form.
//!
//! Test ids: `split-detail` is the lower pane, `split-hint` ("Choose a record to see it
//! here.") shows while nothing is chosen; the list and record views bring their own ids.

use super::{
    RuntimeEnv,
    list::{ListProps, ListView},
    navbar::{NavBarProps, RecordNavBar},
    record::{RecordProps, RecordView},
};
use crate::design::{Form, FormMode};
use crate::forms::actions::NavigationTarget;
use crate::forms::cursor::RecordCursor;
use crate::forms::registry::resolve_form;
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::rc::Rc;

struct Pane {
    mode: FormMode,
    cursor: Option<RecordCursor>,
    record: Entity<RecordView>,
    bar: Option<Entity<RecordNavBar>>,
}

pub struct SplitView {
    env: RuntimeEnv,
    form: Form,
    detail: Form,
    list: Entity<ListView>,
    pane: Option<Pane>,
    dirty: bool,
    on_navigate: Rc<dyn Fn(NavigationTarget, &mut App)>,
}

impl SplitView {
    pub fn new(
        env: RuntimeEnv,
        form: Form,
        params: Map<String, Value>,
        on_navigate: Rc<dyn Fn(NavigationTarget, &mut App)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let detail = form
            .detail_form_id
            .as_deref()
            .filter(|d| !d.is_empty())
            .and_then(|id| resolve_form(env.config(cx), id))
            .unwrap_or_else(|| form.clone());
        let this = cx.weak_entity();
        let (open_to, create_to) = (this.clone(), this);
        let list = cx.new(|cx| {
            ListView::new(
                ListProps {
                    env: env.clone(),
                    form: form.clone(),
                    // The list is mid-update when it calls back, and the pane updates it.
                    on_open: Rc::new(move |id, cursor, cx| {
                        let open_to = open_to.clone();
                        cx.defer(move |cx| {
                            let _ = open_to.update(cx, |t, cx| t.show(id, cursor, cx));
                        });
                    }),
                    on_create: Rc::new(move |cx| {
                        let create_to = create_to.clone();
                        cx.defer(move |cx| {
                            let _ = create_to.update(cx, |t, cx| t.create(cx));
                        });
                    }),
                    params,
                },
                window,
                cx,
            )
        });
        Self { env, form, detail, list, pane: None, dirty: false, on_navigate }
    }

    pub fn list(&self) -> &Entity<ListView> {
        &self.list
    }

    pub fn record(&self) -> Option<&Entity<RecordView>> {
        self.pane.as_ref().map(|p| &p.record)
    }

    pub fn nav_bar(&self) -> Option<Entity<RecordNavBar>> {
        self.pane.as_ref().and_then(|p| p.bar.clone())
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn open_pane(&mut self, mode: FormMode, record_id: Option<Value>, cursor: Option<RecordCursor>, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let (mode_to, close_to, dirty_to, saved_to) = (this.clone(), this.clone(), this.clone(), this);
        let record = cx.new(|cx| {
            RecordView::new(
                RecordProps {
                    env: self.env.clone(),
                    form: self.detail.clone(),
                    mode,
                    record_id: record_id.clone(),
                    link: Map::new(),
                    embedded: false,
                    depth: 0,
                    params: Map::new(),
                    initial_notice: None,
                    on_carry: None,
                    events: None,
                    on_mode: Rc::new(move |next, id, cx| {
                        let _ = mode_to.update(cx, |t, cx| {
                            let next = if matches!(next, FormMode::Edit | FormMode::Create) { next } else { FormMode::Detail };
                            let cursor = t.pane.as_ref().and_then(|p| p.cursor.clone());
                            t.open_pane(next, id, cursor, cx);
                        });
                    }),
                    on_close: Rc::new(move |cx| {
                        let _ = close_to.update(cx, |t, cx| {
                            t.pane = None;
                            t.dirty = false;
                            t.list.update(cx, |l, cx| l.set_selected(None, cx));
                            t.list.update(cx, |l, cx| l.refresh(cx));
                            cx.notify();
                        });
                    }),
                    on_navigate: self.on_navigate.clone(),
                    on_notify: None,
                    on_dirty: Some(Rc::new(move |dirty, cx| {
                        let _ = dirty_to.update(cx, |t, cx| {
                            t.dirty = dirty;
                            cx.notify();
                        });
                    })),
                    on_saved: Some(Rc::new(move |_, cx| {
                        let _ = saved_to.update(cx, |t, cx| t.list.update(cx, |l, cx| l.refresh(cx)));
                    })),
                },
                cx,
            )
        });
        let selected = if mode == FormMode::Create { None } else { cursor.as_ref().map(|c| c.index) };
        self.list.update(cx, |l, cx| l.set_selected(selected, cx));
        self.dirty = false;
        self.pane = Some(Pane { mode, cursor, record, bar: None });
        cx.notify();
    }

    fn show(&mut self, id: Value, cursor: RecordCursor, cx: &mut Context<Self>) {
        if self.dirty {
            return;
        }
        self.open_pane(FormMode::Detail, Some(id), Some(cursor), cx);
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        if self.dirty || !self.detail.modes.contains(&FormMode::Create) {
            return;
        }
        self.open_pane(FormMode::Create, None, None, cx);
    }
}

impl Render for SplitView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let dirty = self.dirty;
        if let Some(pane) = &mut self.pane {
            if pane.mode != FormMode::Create && self.form.navigation_bar {
                let this = cx.weak_entity();
                let props = NavBarProps {
                    env: self.env.clone(),
                    form: self.form.clone(),
                    cursor: pane.cursor.clone(),
                    on_go: Rc::new({
                        let this = this.clone();
                        move |id, cursor, cx| {
                            let _ = this.update(cx, |t, cx| t.show(id, cursor, cx));
                        }
                    }),
                    on_new: self.detail.modes.contains(&FormMode::Create).then(|| {
                        Rc::new(move |cx: &mut App| {
                            let _ = this.update(cx, |t, cx| t.create(cx));
                        }) as Rc<dyn Fn(&mut App)>
                    }),
                    disabled: dirty,
                };
                match &pane.bar {
                    Some(bar) => bar.update(cx, |b, cx| b.set_props(props, cx)),
                    None => pane.bar = Some(cx.new(|cx| RecordNavBar::new(props, cx))),
                }
            }
        }
        let detail = match &self.pane {
            Some(pane) => v_flex()
                .children(pane.bar.clone())
                .child(pane.record.clone())
                .into_any_element(),
            None => widgets::styled_text("split-hint", "Choose a record to see it here.", {
                let color = theme.muted_foreground;
                move |d| d.text_sm().text_color(color).p_4()
            })
            .into_any_element(),
        };
        v_flex()
            .size_full()
            .child(div().flex_1().min_h_0().child(self.list.clone()))
            .child(
                div()
                    .id("split-detail")
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(detail)
                    .test_support(),
            )
    }
}
