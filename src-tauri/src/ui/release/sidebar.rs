//! The Runtime mode's sidebar: the bundle block (or, for a cloud session, the cloud
//! block) and Diagnostics. Shown only in runtime-only windows. Port of the runtime
//! branch of src/shell/AppShell.tsx.
//!
//! The sidebar is created while the shell renders, when the shell cannot be read; its
//! children are therefore built on the sidebar's own first render.

use super::{runtime_bar::RuntimeBar, runtime_diagnostics::RuntimeDiagnostics};
use crate::ui::{prelude::*, shell::Shell};

pub struct RuntimeSidebar {
    shell: Entity<Shell>,
    parts: Option<(AnyView, Entity<RuntimeDiagnostics>)>,
}

impl RuntimeSidebar {
    pub fn new(shell: Entity<Shell>) -> Self {
        Self { shell, parts: None }
    }
}

impl Render for RuntimeSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Studio documents have their own sidebar; only bundle windows show this one.
        if !self.shell.read(cx).doc.runtime_only {
            return div().into_any_element();
        }
        let shell = self.shell.clone();
        let (bar, diagnostics) = self
            .parts
            .get_or_insert_with(|| {
                let bar: AnyView = cx
                    .new(|cx| RuntimeBar::new(shell.clone(), window, cx))
                    .into();
                (bar, cx.new(|cx| RuntimeDiagnostics::new(shell, cx)))
            })
            .clone();
        v_flex()
            .gap_2()
            .child(bar)
            .child(diagnostics)
            .into_any_element()
    }
}
