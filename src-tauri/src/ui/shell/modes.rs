//! Studio modes in display order. `DocumentConfig.active_mode` stores `ModeId::key`.
//! Each feature registers its workspace view (and optional sidebar) in `build_view`
//! and `build_sidebar`.

use super::{Shell, placeholder::ModePlaceholder};
use gpui_kit::assets::IconName;
use gpui_kit::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModeId {
    Data,
    Query,
    Design,
    Reports,
    Dashboards,
    Automation,
    App,
    Run,
}

impl ModeId {
    pub const ALL: [ModeId; 8] = [
        ModeId::Data,
        ModeId::Query,
        ModeId::Design,
        ModeId::Reports,
        ModeId::Dashboards,
        ModeId::Automation,
        ModeId::App,
        ModeId::Run,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ModeId::Data => "data",
            ModeId::Query => "query",
            ModeId::Design => "design",
            ModeId::Reports => "reports",
            ModeId::Dashboards => "dashboards",
            ModeId::Automation => "automation",
            ModeId::App => "app",
            ModeId::Run => "run",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ModeId::Data => "Data",
            ModeId::Query => "Query",
            ModeId::Design => "Design",
            ModeId::Reports => "Reports",
            ModeId::Dashboards => "Dashboards",
            ModeId::Automation => "Automation",
            ModeId::App => "Settings",
            ModeId::Run => "Runtime",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            ModeId::Data => IconName::Database,
            ModeId::Query => IconName::CodeXml,
            ModeId::Design => IconName::Shapes,
            ModeId::Reports => IconName::FileText,
            ModeId::Dashboards => IconName::LayoutDashboard,
            ModeId::Automation => IconName::Zap,
            ModeId::App => IconName::Settings,
            ModeId::Run => IconName::Play,
        }
    }

    /// The mode for a stored key; unknown keys fall back to Data.
    pub fn from_key(key: &str) -> ModeId {
        Self::ALL
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
        ModeId::Data => placeholder(cx, "Data", "Tables and records"),
        ModeId::Query => placeholder(cx, "Query", "SQL and visual queries"),
        ModeId::Design => placeholder(cx, "Form builder", "Forms"),
        ModeId::Reports => placeholder(cx, "Reports", "Printable reports"),
        ModeId::Dashboards => placeholder(cx, "Dashboards", "Charts and KPIs"),
        ModeId::Automation => placeholder(cx, "Automation", "Actions and triggers"),
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
        _ => None,
    }
}

fn placeholder(cx: &mut App, title: &'static str, description: &'static str) -> AnyView {
    cx.new(|_| ModePlaceholder { title, description }).into()
}
