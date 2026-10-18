//! Offered in Data mode right after a table is created (port of `CreateFormsOffer.tsx`):
//! generate its list and detail forms and a navigation item with the same generator as
//! "Generate app from tables". Hidden once any form uses the table.
//!
//! Test ids: `create-forms-offer` (the offer, labelled "Forms for <table>"), `offer-message`,
//! `create-forms` (the button "Create forms for <table>"), `offer-dismiss` ("Not now"),
//! and a success toast "Forms for <table> are ready in Design and Run mode." when done.

use super::generate::{GenerateAppButton, GenerateEvent};
use crate::forms::schema::form_table;
use crate::ui::{prelude::*, shell::Shell, toast::Toast, widgets::Severity};

pub enum OfferEvent {
    Dismissed,
}

pub struct CreateFormsOffer {
    shell: Entity<Shell>,
    table: String,
    done: bool,
    button: Entity<GenerateAppButton>,
    _subscription: Subscription,
}
impl EventEmitter<OfferEvent> for CreateFormsOffer {}

impl CreateFormsOffer {
    pub fn new(shell: Entity<Shell>, table: &str, cx: &mut Context<Self>) -> Self {
        let button = cx.new(|_| {
            GenerateAppButton::new(
                shell.clone(),
                "create-forms",
                format!("Create forms for {table}"),
                Some(vec![table.to_string()]),
            )
        });
        let subscription = cx.subscribe(&button, |this, _, _: &GenerateEvent, cx| {
            this.done = true;
            let toast = Toast::success(format!("Forms for {} are ready in Design and Run mode.", this.table));
            this.shell.update(cx, |s, cx| s.toast(toast, cx));
            cx.emit(OfferEvent::Dismissed);
            cx.notify();
        });
        Self { shell, table: table.to_string(), done: false, button, _subscription: subscription }
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    /// True once any form reads the table.
    fn covered(&self, cx: &App) -> bool {
        let config = self.shell.read(cx).config(cx);
        config.design.forms.iter().any(|form| form_table(form) == Some(self.table.as_str()))
    }
}

impl Render for CreateFormsOffer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.done || self.covered(cx) {
            return div().into_any_element();
        }
        let title = format!("Table {} created. Build its screens now?", self.table);
        widgets::callout("offer-message", Severity::Info, &title, None, cx)
            .id("create-forms-offer")
            .aria_label(format!("Forms for {}", self.table))
            .items_center()
            .child(self.button.clone())
            .child(
                Button::new("offer-dismiss")
                    .ghost()
                    .small()
                    .label("Not now")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OfferEvent::Dismissed))),
            )
            .test_support()
            .into_any_element()
    }
}
