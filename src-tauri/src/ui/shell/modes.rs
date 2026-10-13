//! Studio modes in display order. `DocumentConfig.active_mode` stores `ModeId::key`.
//! Each feature registers its workspace view (and optional sidebar) in `build_view`
//! and `build_sidebar`. Saved queries open on the Data page; documents stored with the
//! former `"query"` mode open there too. `Run` is the whole window of a runtime bundle;
//! Studio runs the app in the Run App window instead (`ui::runtime::window`).

use super::Shell;
use gpui_kit::assets::IconName;
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModeId {
    Data,
    Design,
    Automation,
    App,
    Run,
}

impl ModeId {
    pub const ALL: [ModeId; 5] = [
        ModeId::Data,
        ModeId::Design,
        ModeId::Automation,
        ModeId::App,
        ModeId::Run,
    ];

    /// The modes Studio switches between (Run opens the Run App window instead).
    pub const STUDIO: [ModeId; 4] = [ModeId::Data, ModeId::Design, ModeId::Automation, ModeId::App];

    pub fn key(self) -> &'static str {
        match self {
            ModeId::Data => "data",
            ModeId::Design => "design",
            ModeId::Automation => "automation",
            ModeId::App => "app",
            ModeId::Run => "run",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ModeId::Data => "Data",
            ModeId::Design => "Design",
            ModeId::Automation => "Automation",
            ModeId::App => "Settings",
            ModeId::Run => "Run App",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            ModeId::Data => IconName::Database,
            ModeId::Design => IconName::Shapes,
            ModeId::Automation => IconName::Zap,
            ModeId::App => IconName::Settings,
            ModeId::Run => IconName::Play,
        }
    }

    /// The Studio mode for a stored key; unknown keys (and "run", from before the Run App
    /// window) fall back to Data. Reports and dashboards were modes of their own before
    /// config version 8 and now open Design.
    pub fn from_key(key: &str) -> ModeId {
        if matches!(key, "reports" | "dashboards") {
            return ModeId::Design;
        }
        Self::STUDIO
            .into_iter()
            .find(|m| m.key() == key)
            .unwrap_or(ModeId::Data)
    }

    /// Element id of the mode-switch button (`mode-data`, `mode-app`, ...).
    pub fn button_id(self) -> SharedString {
        format!("mode-{}", self.key()).into()
    }
}

/// The workspace body of a mode.
pub fn build_view(
    mode: ModeId,
    shell: &Entity<Shell>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    match mode {
        ModeId::App => cx
            .new(|cx| super::settings::AppSettings::new(shell.clone(), window, cx))
            .into(),
        ModeId::Data => cx
            .new(|cx| crate::ui::data::workbench::DataMode::new(shell.clone(), window, cx))
            .into(),
        ModeId::Design => cx
            .new(|cx| crate::ui::pages::DesignMode::new(shell.clone(), window, cx))
            .into(),
        ModeId::Automation => cx
            .new(|cx| crate::ui::automation::AutomationMode::new(shell.clone(), window, cx))
            .into(),
        ModeId::Run => cx
            .new(|cx| crate::ui::runtime::RunMode::new(shell.clone(), window, cx))
            .into(),
    }
}

/// Optional sidebar content shown under the mode switch.
pub fn build_sidebar(
    mode: ModeId,
    shell: &Entity<Shell>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyView> {
    match mode {
        ModeId::Data => Some(
            cx.new(|cx| super::object_browser::ObjectBrowser::new(shell.clone(), window, cx))
                .into(),
        ),
        // Runtime-only windows (bundles) show the bundle block and Diagnostics; in
        // Studio the view renders nothing.
        ModeId::Run => Some(
            cx.new(|_| crate::ui::release::sidebar::RuntimeSidebar::new(shell.clone()))
                .into(),
        ),
        _ => None,
    }
}
