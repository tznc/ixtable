//! Renders a form in list, continuous, split, detail, create or edit mode (port of
//! `FormRenderer.tsx`). Navigation between list and detail stays inside the renderer (with a
//! back link), so it also works embedded in dashboards and the Design studio's preview; in Run
//! mode (`RuntimeEnv::runtime` set) a navigation target opens as a Runtime page instead.
//!
//! The renderer is a stack of views: the top one is shown, `close` pops it (or leaves through
//! `on_done` / the back link at the bottom).
//!
//! Test ids: `rt-form` is the renderer, `form-missing` / `form-denied` replace it when the form
//! does not exist or the role may not read it; the views bring their own ids.

use super::{
    BackLink, RuntimeEnv,
    back::back_crumb,
    continuous::ContinuousView,
    list::{ListProps, ListView},
    navbar::{NavBarProps, RecordNavBar},
    record::{NotifyFn, RecordProps, RecordView},
    split::SplitView,
};
use crate::design::{Form, FormMode};
use crate::forms::actions::NavigationTarget;
use crate::forms::cursor::RecordCursor;
use crate::forms::navigation::{PageKind, RuntimePage};
use crate::forms::rbac::Operation;
use crate::forms::registry::{is_designed_form, resolve_form};
use crate::forms::values::RecordValues;
use crate::ui::prelude::*;
use serde_json::{Map, Value};
use std::rc::Rc;

#[derive(Clone)]
pub struct FormRendererProps {
    pub env: RuntimeEnv,
    pub form_id: String,
    pub mode: Option<FormMode>,
    pub record_id: Option<Value>,
    /// Fixed foreign-key values for a child form opened from a related-record list.
    pub link: Map<String, Value>,
    /// Embedded child form: no related lists of its own beyond the nesting limit.
    pub embedded: bool,
    /// Related-list levels above this form.
    pub depth: usize,
    /// Called when the form is closed from its first view (embedded or after delete).
    pub on_done: Option<Rc<dyn Fn(&mut App)>>,
    /// Back link to the previous runtime page, shown while the form is on its first view.
    pub back: Option<BackLink>,
    /// Receives an embedded form's messages (it closes after saving).
    pub on_notify: Option<NotifyFn>,
    /// Page parameters (navigation `params`, dashboard filter values) for query bindings.
    pub params: Map<String, Value>,
    /// Told whether the shown record has unsaved edits.
    pub on_dirty: Option<Rc<dyn Fn(bool, &mut App)>>,
    /// Receives the saved values after a create or edit commits (popup forms).
    pub on_saved: Option<Rc<dyn Fn(RecordValues, &mut App)>>,
}

impl FormRendererProps {
    pub fn new(env: RuntimeEnv, form_id: impl Into<String>) -> Self {
        Self {
            env,
            form_id: form_id.into(),
            mode: None,
            record_id: None,
            link: Map::new(),
            embedded: false,
            depth: 0,
            on_done: None,
            back: None,
            on_notify: None,
            params: Map::new(),
            on_dirty: None,
            on_saved: None,
        }
    }
}

/// `cursor`: the record's place in the list it was opened from (record navigation bar).
#[derive(Clone)]
struct Entry {
    form_id: String,
    mode: FormMode,
    record_id: Option<Value>,
    cursor: Option<RecordCursor>,
}

enum Shown {
    List(Entity<ListView>),
    Continuous(Entity<ContinuousView>),
    Split(Entity<SplitView>),
    Record(Entity<RecordView>, Option<Entity<RecordNavBar>>),
}

pub struct FormRenderer {
    props: FormRendererProps,
    stack: Vec<Entry>,
    /// The built view and the signature (entry and form definition) it was built for.
    shown: Option<(String, Shown)>,
    dirty: bool,
    _observer: Subscription,
}

impl FormRenderer {
    pub fn new(props: FormRendererProps, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = resolve_form(props.env.config(cx), &props.form_id);
        let mode = props.mode.or_else(|| initial.as_ref().and_then(|f| f.modes.first().copied())).unwrap_or_default();
        let entry = Entry { form_id: props.form_id.clone(), mode, record_id: props.record_id.clone(), cursor: None };
        let store = props.env.store.clone();
        // A definition edit rebuilds the shown view (the Design studio's live preview).
        let observer = cx.observe(&store, |_, _, cx| cx.notify());
        Self { props, stack: vec![entry], shown: None, dirty: false, _observer: observer }
    }

    fn top(&self) -> &Entry {
        self.stack.last().expect("the stack always holds a view")
    }

    pub fn mode(&self) -> FormMode {
        self.top().mode
    }

    pub fn depth_of_stack(&self) -> usize {
        self.stack.len()
    }

    /// The shown list view, if the top view is a list.
    pub fn list(&self) -> Option<Entity<ListView>> {
        match &self.shown {
            Some((_, Shown::List(list))) => Some(list.clone()),
            _ => None,
        }
    }

    /// The shown record view, if the top view is a record.
    pub fn record(&self) -> Option<Entity<RecordView>> {
        match &self.shown {
            Some((_, Shown::Record(record, _))) => Some(record.clone()),
            _ => None,
        }
    }

    pub fn nav_bar(&self) -> Option<Entity<RecordNavBar>> {
        match &self.shown {
            Some((_, Shown::Record(_, bar))) => bar.clone(),
            _ => None,
        }
    }

    fn push(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.stack.push(entry);
        self.reset(cx);
    }

    fn replace(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.stack.pop();
        self.stack.push(entry);
        self.reset(cx);
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.shown = None;
        self.set_dirty(false, cx);
        cx.notify();
    }

    fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if self.dirty != dirty {
            self.dirty = dirty;
            cx.notify();
        }
        if let Some(on_dirty) = self.props.on_dirty.clone() {
            cx.defer(move |cx| on_dirty(dirty, cx));
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.reset(cx);
            return;
        }
        if let Some(done) = self.props.on_done.clone() {
            done(cx);
        } else if let Some(back) = self.props.back.clone() {
            (back.on_back)(cx);
        }
    }

    fn navigate(&mut self, target: NavigationTarget, cx: &mut Context<Self>) {
        if let (Some(runtime), false) = (self.props.env.runtime.clone(), self.props.embedded) {
            let Some(kind) = PageKind::parse(&target.kind) else { return };
            let page = RuntimePage {
                kind,
                id: target.id,
                mode: target.mode,
                record_id: target.record_id,
                params: target.params,
                nav_id: None,
            };
            runtime.update(cx, |r, cx| r.navigate(page, cx));
            return;
        }
        if target.kind == "form" {
            let mode = match target.mode.as_deref() {
                Some("detail") => FormMode::Detail,
                Some("create") => FormMode::Create,
                Some("edit") => FormMode::Edit,
                Some("continuous") => FormMode::Continuous,
                Some("split") => FormMode::Split,
                _ => FormMode::List,
            };
            self.push(Entry { form_id: target.id, mode, record_id: target.record_id, cursor: None }, cx);
        }
    }

    fn can_read(&self, form: &Form, cx: &App) -> bool {
        let config = self.props.env.config(cx);
        let subject = if is_designed_form(config, form) {
            ("form", form.id.clone())
        } else {
            ("table", form.source.as_ref().and_then(|s| s.table.clone()).unwrap_or_default())
        };
        self.props.env.can(subject.0, &subject.1, Operation::Read, cx)
    }

    fn build(&mut self, entry: &Entry, form: &Form, window: &mut Window, cx: &mut Context<Self>) -> Shown {
        let this = cx.entity();
        let env = self.props.env.clone();
        let params = self.props.params.clone();
        let detail_id = form.detail_form_id.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| form.id.clone());
        match entry.mode {
            FormMode::List => {
                let (open, create) = (this.clone(), this.clone());
                let detail = detail_id.clone();
                let detail_for_create = detail_id.clone();
                Shown::List(cx.new(|cx| {
                    ListView::new(
                        ListProps {
                            env,
                            form: form.clone(),
                            on_open: Rc::new(move |id, cursor, cx| {
                                let entry = Entry {
                                    form_id: detail.clone(),
                                    mode: FormMode::Detail,
                                    record_id: Some(id),
                                    cursor: Some(cursor),
                                };
                                open.update(cx, |t, cx| t.push(entry, cx));
                            }),
                            on_create: Rc::new(move |cx| {
                                let entry = Entry {
                                    form_id: detail_for_create.clone(),
                                    mode: FormMode::Create,
                                    record_id: None,
                                    cursor: None,
                                };
                                create.update(cx, |t, cx| t.push(entry, cx));
                            }),
                            params,
                        },
                        window,
                        cx,
                    )
                }))
            }
            FormMode::Continuous => {
                Shown::Continuous(cx.new(|cx| ContinuousView::new(env, form.clone(), params, window, cx)))
            }
            FormMode::Split => {
                let navigate = this.clone();
                Shown::Split(cx.new(|cx| {
                    SplitView::new(
                        env,
                        form.clone(),
                        params,
                        Rc::new(move |target, cx| navigate.update(cx, |t, cx| t.navigate(target, cx))),
                        window,
                        cx,
                    )
                }))
            }
            FormMode::Detail | FormMode::Create | FormMode::Edit => {
                let (mode_to, close_to, nav_to, dirty_to) = (this.clone(), this.clone(), this.clone(), this.clone());
                let (form_id, cursor) = (form.id.clone(), entry.cursor.clone());
                let record = cx.new(|cx| {
                    RecordView::new(
                        RecordProps {
                            env: env.clone(),
                            form: form.clone(),
                            mode: entry.mode,
                            record_id: entry.record_id.clone(),
                            link: self.props.link.clone(),
                            embedded: self.props.embedded,
                            depth: self.props.depth,
                            params: params.clone(),
                            on_mode: Rc::new(move |mode, id, cx| {
                                let entry = Entry { form_id: form_id.clone(), mode, record_id: id, cursor: cursor.clone() };
                                mode_to.update(cx, |t, cx| t.replace(entry, cx));
                            }),
                            on_close: Rc::new(move |cx| {
                                close_to.update(cx, |t, cx| t.close(cx));
                            }),
                            on_navigate: Rc::new(move |target, cx| {
                                nav_to.update(cx, |t, cx| t.navigate(target, cx));
                            }),
                            on_notify: self.props.on_notify.clone(),
                            on_dirty: Some(Rc::new(move |dirty, cx| {
                                dirty_to.update(cx, |t, cx| t.set_dirty(dirty, cx));
                            })),
                            on_saved: self.props.on_saved.clone(),
                        },
                        cx,
                    )
                });
                Shown::Record(record, None)
            }
        }
    }

    fn crumb(&self, cx: &mut Context<Self>) -> Option<BackLink> {
        if self.props.embedded {
            return None;
        }
        if self.stack.len() > 1 {
            let previous = &self.stack[self.stack.len() - 2];
            let label = resolve_form(self.props.env.config(cx), &previous.form_id)
                .map_or_else(|| "previous view".to_string(), |f| f.name);
            let this = cx.entity();
            return Some(BackLink::new(label, move |cx| {
                this.update(cx, |t, cx| t.close(cx));
            }));
        }
        self.props.back.clone()
    }
}

fn signature(entry: &Entry, form: &Form) -> String {
    serde_json::json!([entry.form_id, entry.record_id, format!("{:?}", entry.mode), form]).to_string()
}

impl Render for FormRenderer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry = self.top().clone();
        let Some(form) = resolve_form(self.props.env.config(cx), &entry.form_id) else {
            return super::message("form-missing", "This form does not exist.", true, cx).into_any_element();
        };
        if !self.can_read(&form, cx) {
            return super::message("form-denied", format!("You do not have access to {}.", form.name), true, cx)
                .into_any_element();
        }
        let sig = signature(&entry, &form);
        if self.shown.as_ref().is_none_or(|(s, _)| *s != sig) {
            let built = self.build(&entry, &form, window, cx);
            self.shown = Some((sig, built));
        }
        // The bar walks the list the record was opened from, else this form's own records.
        if let Some((_, Shown::Record(_, bar))) = &mut self.shown {
            let list_form = entry
                .cursor
                .as_ref()
                .and_then(|c| resolve_form(self.props.env.config(cx), &c.form_id))
                .unwrap_or_else(|| form.clone());
            let show_bar = form.navigation_bar && entry.mode == FormMode::Detail && form.source.is_some();
            let can_create = form.modes.contains(&FormMode::Create)
                && form.source.as_ref().is_some_and(|s| s.kind == crate::design::SourceKind::Table)
                && self.props.env.can(
                    if is_designed_form(self.props.env.config(cx), &form) { "form" } else { "table" },
                    &if is_designed_form(self.props.env.config(cx), &form) {
                        form.id.clone()
                    } else {
                        form.source.as_ref().and_then(|s| s.table.clone()).unwrap_or_default()
                    },
                    Operation::Create,
                    cx,
                );
            if show_bar {
                let this = cx.entity();
                let (go_to, new_to) = (this.clone(), this.clone());
                let form_id = form.id.clone();
                let new_form_id = form.id.clone();
                let props = NavBarProps {
                    env: self.props.env.clone(),
                    form: list_form,
                    cursor: entry.cursor.clone(),
                    on_go: Rc::new(move |id, cursor, cx| {
                        let entry = Entry {
                            form_id: form_id.clone(),
                            mode: FormMode::Detail,
                            record_id: Some(id),
                            cursor: Some(cursor),
                        };
                        go_to.update(cx, |t, cx| t.replace(entry, cx));
                    }),
                    on_new: can_create.then(|| {
                        Rc::new(move |cx: &mut App| {
                            let entry = Entry {
                                form_id: new_form_id.clone(),
                                mode: FormMode::Create,
                                record_id: None,
                                cursor: None,
                            };
                            new_to.update(cx, |t, cx| t.push(entry, cx));
                        }) as Rc<dyn Fn(&mut App)>
                    }),
                    disabled: self.dirty,
                };
                match bar {
                    Some(existing) => existing.update(cx, |b, cx| b.set_props(props, cx)),
                    None => *bar = Some(cx.new(|cx| RecordNavBar::new(props, cx))),
                }
            }
        }
        let crumb = self.crumb(cx);
        let body: AnyElement = match &self.shown {
            Some((_, Shown::List(v))) => v.clone().into_any_element(),
            Some((_, Shown::Continuous(v))) => v.clone().into_any_element(),
            Some((_, Shown::Split(v))) => v.clone().into_any_element(),
            Some((_, Shown::Record(v, bar))) => v_flex()
                .size_full()
                .children(bar.clone())
                .child(v.clone())
                .into_any_element(),
            None => div().into_any_element(),
        };
        v_flex()
            .id("rt-form")
            .size_full()
            .children(crumb.map(|c| back_crumb(&c)))
            .child(body)
            .into_any_element()
    }
}
