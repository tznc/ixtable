//! The Runtime mode's sidebar: the bundle block (or, for a cloud session, the cloud
//! block) and Diagnostics. Shown only in runtime-only windows. Port of the runtime
//! branch of src/shell/AppShell.tsx.
//!
//! The sidebar is created while the shell renders, when the shell cannot be read; its
//! children are therefore built on the sidebar's own first render.

use super::{runtime_bar::RuntimeBar, runtime_diagnostics::RuntimeDiagnostics};
use crate::ui::{
    cloud::{runtime_bar::CloudRuntimeBar, session::CloudRuntime},
    prelude::*,
    shell::Shell,
};

pub struct RuntimeSidebar {
    shell: Entity<Shell>,
    bundle: Option<AnyView>,
    cloud: Option<AnyView>,
    diagnostics: Option<Entity<RuntimeDiagnostics>>,
}

impl RuntimeSidebar {
    pub fn new(shell: Entity<Shell>) -> Self {
        Self { shell, bundle: None, cloud: None, diagnostics: None }
    }
}

impl Render for RuntimeSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Studio documents have their own sidebar; only bundle windows show this one.
        if !self.shell.read(cx).doc.runtime_only {
            return div().into_any_element();
        }
        let shell = self.shell.clone();
        // A cloud installation shows the cloud block in place of the bundle block.
        let session_id = shell.read(cx).doc.session_id.clone();
        let is_cloud = CloudRuntime::global(cx)
            .read(cx)
            .active()
            .is_some_and(|a| a.session_id == session_id);
        let block = if is_cloud {
            self.cloud
                .get_or_insert_with(|| {
                    let shell = shell.clone();
                    cx.new(|cx| CloudRuntimeBar::new(shell, cx)).into()
                })
                .clone()
        } else {
            self.bundle
                .get_or_insert_with(|| {
                    let shell = shell.clone();
                    cx.new(|cx| RuntimeBar::new(shell, window, cx)).into()
                })
                .clone()
        };
        let diagnostics = self
            .diagnostics
            .get_or_insert_with(|| cx.new(|cx| RuntimeDiagnostics::new(shell, cx)))
            .clone();
        v_flex().gap_2().child(block).child(diagnostics).into_any_element()
    }
}
