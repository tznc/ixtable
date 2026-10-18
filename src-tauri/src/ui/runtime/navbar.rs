//! Access-style record navigation (port of `RecordNavBar.tsx`): first, previous, next, last
//! and new, with the shown record's position. Positions are read through the list's own page
//! reader (`record_at`), so the order is the list's sort and search.
//!
//! Test ids: `record-nav` ("Record navigation"), `nav-first`, `nav-previous`, `nav-next`,
//! `nav-last`, `nav-new`, `nav-position` ("Record 2 of 5", "5 records", "Loading…") and
//! `nav-error`. Whether a button can act is read from `can_first` / `can_previous` / `can_next`
//! / `can_last` / `can_new`.

use super::RuntimeEnv;
use crate::design::Form;
use crate::forms::cursor::{RecordCursor, record_at};
use crate::ui::events::DocEvent;
use crate::ui::prelude::*;
use serde_json::Value;
use std::rc::Rc;

pub type GoHandler = Rc<dyn Fn(Value, RecordCursor, &mut App)>;

#[derive(Clone)]
pub struct NavBarProps {
    pub env: RuntimeEnv,
    /// The form whose records the bar moves through (the list form).
    pub form: Form,
    /// Position of the shown record; None when it was opened without a list.
    pub cursor: Option<RecordCursor>,
    pub on_go: GoHandler,
    /// Opens a new record; None when the role or form cannot create.
    pub on_new: Option<Rc<dyn Fn(&mut App)>>,
    /// Blocks moving away (unsaved edits, a running save).
    pub disabled: bool,
}

pub struct RecordNavBar {
    props: NavBarProps,
    total: Option<u64>,
    busy: bool,
    error: String,
    order: String,
    seq: u64,
    _bus: Subscription,
}

fn order_key(props: &NavBarProps) -> String {
    let cursor = props.cursor.as_ref();
    format!(
        "{}|{:?}|{:?}|{:?}",
        props.form.id,
        cursor.map(|c| &c.sorts),
        cursor.map(|c| &c.filters),
        cursor.map(|c| &c.params)
    )
}

impl RecordNavBar {
    pub fn new(props: NavBarProps, cx: &mut Context<Self>) -> Self {
        let bus = props.env.bus.clone();
        let subscription = cx.subscribe(&bus, |this, _, event: &DocEvent, cx| {
            if matches!(event, DocEvent::RecordsChanged | DocEvent::DatabaseChanged) {
                this.load_total(cx);
            }
        });
        let mut bar = Self {
            order: order_key(&props),
            props,
            total: None,
            busy: false,
            error: String::new(),
            seq: 0,
            _bus: subscription,
        };
        bar.load_total(cx);
        bar
    }

    /// Takes the latest props; the total is read again when the order changed.
    pub fn set_props(&mut self, props: NavBarProps, cx: &mut Context<Self>) {
        let key = order_key(&props);
        self.props = props;
        if key != self.order {
            self.order = key;
            self.total = None;
            self.load_total(cx);
        }
        cx.notify();
    }

    fn query(&self, cx: &App) -> (String, Form, RecordCursor, serde_json::Map<String, Value>) {
        let cursor = self.props.cursor.clone().unwrap_or(RecordCursor {
            form_id: self.props.form.id.clone(),
            index: 0,
            sorts: vec![],
            filters: vec![],
            params: Default::default(),
        });
        (self.props.env.label.to_string(), self.props.form.clone(), cursor, self.props.env.app(cx))
    }

    fn load_total(&mut self, cx: &mut Context<Self>) {
        self.seq += 1;
        let seq = self.seq;
        let (label, form, cursor, app) = self.query(cx);
        task::spawn_call(
            cx,
            move || record_at(&label, &form, &cursor, 0, &app),
            move |this, found, cx| {
                if this.seq == seq {
                    this.total = found.ok().map(|f| f.total);
                    cx.notify();
                }
            },
        );
    }

    fn index(&self) -> Option<usize> {
        self.props.cursor.as_ref().map(|c| c.index)
    }

    fn last(&self) -> i64 {
        self.total.map_or(-1, |t| t as i64 - 1)
    }

    fn blocked(&self) -> bool {
        self.props.disabled || self.busy || self.total.is_none()
    }

    pub fn can_first(&self) -> bool {
        !self.blocked() && self.total.unwrap_or(0) > 0 && self.index() != Some(0)
    }

    pub fn can_previous(&self) -> bool {
        !self.blocked() && self.index().is_some_and(|i| i > 0)
    }

    pub fn can_next(&self) -> bool {
        !self.blocked() && self.index().is_some_and(|i| i as i64 <= self.last() - 1)
    }

    pub fn can_last(&self) -> bool {
        !self.blocked() && self.total.unwrap_or(0) > 0 && self.index().map(|i| i as i64) != Some(self.last())
    }

    pub fn can_new(&self) -> bool {
        self.props.on_new.is_some() && !self.props.disabled
    }

    pub fn position(&self) -> String {
        match (self.total, self.index()) {
            (None, _) => "Loading…".into(),
            (Some(total), None) => widgets::plural(total as usize, "record"),
            (Some(total), Some(index)) => format!("Record {} of {total}", (index as u64 + 1).min(total)),
        }
    }

    fn go(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy || self.total.is_none() {
            return;
        }
        self.busy = true;
        self.error.clear();
        let (label, form, mut cursor, app) = self.query(cx);
        cursor.index = index;
        let on_go = self.props.on_go.clone();
        task::spawn_call(
            cx,
            move || record_at(&label, &form, &cursor, index, &app).map(|found| (found, cursor)),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok((found, cursor)) if !found.record_id.is_null() => on_go(found.record_id, cursor, cx),
                    Ok(_) => {}
                    Err(e) => this.error = e.message,
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for RecordNavBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, icon: IconName, enabled: bool| {
            Button::new(id).ghost().small().icon(icon).disabled(!enabled)
        };
        let index = self.index();
        let last = self.last().max(0) as usize;
        h_flex()
            .id("record-nav")
            .aria_label("Record navigation")
            .gap_1()
            .items_center()
            .child(
                button("nav-first", IconName::ChevronsLeft, self.can_first())
                    .on_click(cx.listener(|this, _, _, cx| this.go(0, cx))),
            )
            .child(
                button("nav-previous", IconName::ChevronLeft, self.can_previous())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(i) = index {
                            this.go(i.saturating_sub(1), cx);
                        }
                    })),
            )
            .child(widgets::text("nav-position", self.position()))
            .child(
                button("nav-next", IconName::ChevronRight, self.can_next())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(i) = index {
                            this.go(i + 1, cx);
                        }
                    })),
            )
            .child(
                button("nav-last", IconName::ChevronsRight, self.can_last())
                    .on_click(cx.listener(move |this, _, _, cx| this.go(last, cx))),
            )
            .children(self.props.on_new.clone().map(|on_new| {
                button("nav-new", IconName::Plus, self.can_new()).on_click(move |_, _, cx| on_new(cx))
            }))
            .when(!self.error.is_empty(), |d| d.child(super::message("nav-error", self.error.clone(), true, cx)))
            .test_support()
    }
}
