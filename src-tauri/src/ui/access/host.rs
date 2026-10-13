//! Hosts the Access import wizard on the start screen: the start screen's "Import
//! Access database" button opens it, and an imported document opens in the window.

use super::wizard::{AccessImportWizard, WizardEvent};
use crate::ui::{prelude::*, start::StartScreen};

pub struct AccessHost {
    label: String,
    screen: WeakEntity<StartScreen>,
    wizard: Option<Entity<AccessImportWizard>>,
}

impl AccessHost {
    pub fn new(label: String, screen: WeakEntity<StartScreen>) -> Self {
        Self { label, screen, wizard: None }
    }

    pub fn is_open(&self) -> bool {
        self.wizard.is_some()
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if self.wizard.is_some() {
            return;
        }
        let label = self.label.clone();
        let wizard = cx.new(|cx| AccessImportWizard::new(label, cx));
        cx.subscribe(&wizard, |this, _, event: &WizardEvent, cx| {
            this.wizard = None;
            if let WizardEvent::Opened(state) = event {
                let state = state.clone();
                let _ = this.screen.update(cx, |screen, cx| {
                    screen.run("Opening document…", move |_| Ok(state), cx)
                });
            }
            cx.notify();
        })
        .detach();
        self.wizard = Some(wizard);
        cx.notify();
    }
}

impl Render for AccessHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().children(self.wizard.clone())
    }
}
