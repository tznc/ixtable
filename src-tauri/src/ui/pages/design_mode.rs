//! The Design mode (`docs/decisions/pages.md`): one place to build pages. The sidebar lists
//! the pages, the navigation editor, the generators and any form or report that is on no
//! page. The body shows the chosen page (`PagePanel`), or a form or report of a page opened
//! from it in the form designer (`DesignStudio`) or the report designer (`ReportPanel`),
//! with a link back to the page.
//!
//! Every reveal of the Design mode (Problems links, "Edit form", "Edit report") is taken
//! here and handed to the panel that shows the object.
//!
//! Test ids: `design-mode`, `design-sidebar`, `new-page`, `new-form`, `new-report`, `page-item-N`, `no-pages`,
//! `design-navigation`, `generate-table`, `generate-form`, `generate-app`,
//! `generate-notice`, `generate-error`, `loose-form:<id>`, `loose-report:<id>`,
//! `back-to-page`, `design-crumb`, `duplicate-form`, `delete-form`.

use super::PagePanel;
use crate::pages::{ComponentKind, Page};
use crate::ui::{
    controls::choice,
    design::{generate_app, DesignStudio, View},
    prelude::*,
    reports::ReportPanel,
    shell::{ModeId, RevealTarget, Shell},
};

/// What the Design mode shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Page,
    Form,
    Report,
    Navigation,
}

#[derive(Default)]
struct Generate {
    table: String,
    notice: String,
    error: String,
}

pub struct DesignMode {
    shell: Entity<Shell>,
    pub focus: Focus,
    pub pages: Entity<PagePanel>,
    pub forms: Entity<DesignStudio>,
    pub reports: Entity<ReportPanel>,
    generate: Generate,
    _subscriptions: Vec<Subscription>,
}

impl DesignMode {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pages = cx.new(|cx| PagePanel::new(shell.clone(), window, cx));
        let forms = cx.new(|cx| DesignStudio::new(shell.clone(), window, cx));
        let reports = cx.new(|cx| ReportPanel::new(shell.clone(), window, cx));
        let subscriptions = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe(&pages, |_, _, cx| cx.notify()),
            cx.observe(&forms, |_, _, cx| cx.notify()),
            cx.observe(&reports, |_, _, cx| cx.notify()),
        ];
        Self { shell, focus: Focus::Page, pages, forms, reports, generate: Generate::default(), _subscriptions: subscriptions }
    }

    fn config<'a>(&self, cx: &'a App) -> &'a crate::archive::DocumentConfig {
        self.shell.read(cx).config(cx)
    }

    /// Opens what another view asked to reveal.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(ModeId::Design));
        match request {
            Some(RevealTarget::Design { object_id, element_id }) => self.open_form(&object_id, element_id, cx),
            Some(RevealTarget::Reports { object_id, element_id }) => self.open_report(&object_id, element_id, cx),
            Some(RevealTarget::Page { object_id, element_id }) => self.open_page(&object_id, element_id, cx),
            _ => {}
        }
    }

    pub fn open_page(&mut self, id: &str, element: Option<String>, cx: &mut Context<Self>) {
        self.focus = Focus::Page;
        self.pages.update(cx, |p, cx| p.open(id, element, cx));
        cx.notify();
    }

    /// Opens a form in the form designer; the back link returns to a page showing it.
    pub fn open_form(&mut self, id: &str, element: Option<String>, cx: &mut Context<Self>) {
        self.remember_page(ComponentKind::Form, id, cx);
        self.focus = Focus::Form;
        let id = id.to_string();
        self.forms.update(cx, |f, cx| {
            f.view = View::Form;
            f.open(id, element, cx);
        });
        cx.notify();
    }

    pub fn open_report(&mut self, id: &str, element: Option<String>, cx: &mut Context<Self>) {
        self.remember_page(ComponentKind::Report, id, cx);
        self.focus = Focus::Report;
        self.reports.update(cx, |r, cx| r.open(id, element, cx));
        cx.notify();
    }

    /// Selects a page that shows form or report `id` (keeping the current one if it does).
    fn remember_page(&mut self, kind: ComponentKind, id: &str, cx: &mut Context<Self>) {
        let shows = |p: &Page| {
            p.components.iter().any(|c| {
                c.kind == kind && [c.form_id.as_deref(), c.report_id.as_deref()].contains(&Some(id))
            })
        };
        let current = self.pages.read(cx).current(cx);
        if current.as_ref().is_some_and(|p| shows(p)) {
            return;
        }
        if let Some(page) = self.config(cx).pages.iter().find(|p| shows(p)).map(|p| p.id.clone()) {
            self.pages.update(cx, |p, cx| p.select(&page, cx));
        }
    }

    pub fn show_navigation(&mut self, cx: &mut Context<Self>) {
        self.focus = Focus::Navigation;
        self.forms.update(cx, |f, cx| f.set_view(View::Navigation, cx));
        cx.notify();
    }

    fn back_to_page(&mut self, cx: &mut Context<Self>) {
        self.focus = Focus::Page;
        cx.notify();
    }

    fn generate(&mut self, table: Option<String>, cx: &mut Context<Self>) {
        let (store, label) = {
            let shell = self.shell.read(cx);
            (shell.store.clone(), shell.label().to_string())
        };
        generate_app(store, label, table.map(|t| vec![t]), cx, |this, result, cx| {
            match result {
                Ok(added) => {
                    this.generate.error.clear();
                    this.generate.notice = if added > 0 {
                        format!("Added {added} forms and navigation items, each form on a page.")
                    } else {
                        "Every table already has forms.".to_string()
                    };
                }
                Err(e) => this.generate.error = e.message,
            }
            cx.notify();
        });
    }

    /// The focus to show: a form or report that no longer exists falls back to its page.
    fn shown_focus(&mut self, cx: &mut Context<Self>) -> Focus {
        let config = self.config(cx);
        let gone = match self.focus {
            Focus::Form => self.forms.read(cx).form_id.as_ref().is_none_or(|id| !config.design.forms.iter().any(|f| &f.id == id)),
            Focus::Report => self.reports.read(cx).selected.as_ref().is_none_or(|id| !config.reports.iter().any(|r| &r.id == id)),
            _ => false,
        };
        if gone {
            self.focus = Focus::Page;
        }
        self.focus
    }

    fn render_sidebar(&mut self, focus: Focus, cx: &mut Context<Self>) -> impl IntoElement {
        let config = self.config(cx).clone();
        let theme = cx.theme();
        let (border, muted, danger) = (theme.border, theme.muted_foreground, theme.danger);
        let current = self.pages.read(cx).current(cx).map(|p| p.id);
        let heading = |text: &'static str| div().pt_2().text_xs().text_color(muted).child(text);
        let tables: Vec<String> =
            self.shell.read(cx).objects.iter().filter(|o| o.object_type == "table").map(|o| o.name.clone()).collect();
        let table = Some(self.generate.table.clone()).filter(|t| tables.contains(t)).or_else(|| tables.first().cloned());
        let entity = cx.entity();
        let placed = |kind: ComponentKind, id: &str| {
            config.pages.iter().any(|p| {
                p.components.iter().any(|c| c.kind == kind && [c.form_id.as_deref(), c.report_id.as_deref()].contains(&Some(id)))
            })
        };
        let loose_forms: Vec<_> = config.design.forms.iter().filter(|f| !placed(ComponentKind::Form, &f.id)).cloned().collect();
        let loose_reports: Vec<_> = config.reports.iter().filter(|r| !placed(ComponentKind::Report, &r.id)).cloned().collect();
        let no_tables = table.is_none();
        v_flex()
            .id("design-sidebar")
            .aria_label("Pages")
            .w(px(230.))
            .h_full()
            .flex_none()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(border)
            .overflow_y_scroll()
            .child(heading("PAGES"))
            .child(
                Button::new("new-page")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("New page")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.pages.update(cx, |p, cx| p.create(cx));
                        this.focus = Focus::Page;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("new-form")
                            .outline()
                            .small()
                            .label("New form")
                            .tooltip("A form on a page of its own")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.forms.update(cx, |f, cx| f.create_form(cx));
                                this.open_form(&id, None, cx);
                            })),
                    )
                    .child(
                        Button::new("new-report")
                            .outline()
                            .small()
                            .label("New report")
                            .tooltip("A report on a page of its own")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.reports.update(cx, |r, cx| r.create(cx));
                                this.open_report(&id, None, cx);
                            })),
                    ),
            )
            .children(config.pages.iter().enumerate().map(|(i, page)| {
                let id = page.id.clone();
                Button::new(SharedString::from(format!("page-item-{}", i + 1)))
                    .ghost()
                    .small()
                    .start_label(
                        Some(IconName::LayoutDashboard.into()),
                        if page.name.is_empty() { "Untitled page".to_string() } else { page.name.clone() },
                    )
                    .selected(focus != Focus::Navigation && current.as_deref() == Some(page.id.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_page(&id, None, cx)))
            }))
            .when(config.pages.is_empty(), |el| el.child(widgets::text("no-pages", "No pages yet.")))
            .child(
                Button::new("design-navigation")
                    .ghost()
                    .small()
                    .start_label(Some(IconName::Menu.into()), "Navigation")
                    .selected(focus == Focus::Navigation)
                    .on_click(cx.listener(|this, _, _, cx| this.show_navigation(cx))),
            )
            .child(heading("GENERATE"))
            .child(widgets::field(
                "Table to generate from",
                choice(
                    "generate-table",
                    table.as_deref().unwrap_or(""),
                    tables.iter().map(|t| (t.clone(), t.clone())).collect(),
                    "No tables",
                    move |value, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.generate.table = value.to_string();
                            cx.notify();
                        })
                    },
                ),
                cx,
            ))
            .child(
                Button::new("generate-form")
                    .outline()
                    .small()
                    .icon(IconName::WandSparkles)
                    .label("Generate pages from table")
                    .disabled(no_tables)
                    .on_click(cx.listener(move |this, _, _, cx| this.generate(table.clone(), cx))),
            )
            .child(
                Button::new("generate-app")
                    .outline()
                    .small()
                    .icon(IconName::WandSparkles)
                    .label("Generate app from tables")
                    .disabled(no_tables)
                    .on_click(cx.listener(|this, _, _, cx| this.generate(None, cx))),
            )
            .when(!self.generate.notice.is_empty(), |el| {
                el.child(widgets::styled_text("generate-notice", self.generate.notice.clone(), |d| d.text_xs()))
            })
            .when(!self.generate.error.is_empty(), |el| {
                el.child(widgets::styled_text("generate-error", self.generate.error.clone(), move |d| d.text_xs().text_color(danger)))
            })
            .when(!loose_forms.is_empty() || !loose_reports.is_empty(), |el| {
                el.child(heading("NOT ON A PAGE"))
                    .children(loose_forms.into_iter().map(|f| {
                        let id = f.id.clone();
                        Button::new(SharedString::from(format!("loose-form:{}", f.id)))
                            .ghost()
                            .small()
                            .start_label(Some(IconName::LayoutDashboard.into()), f.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.open_form(&id, None, cx)))
                    }))
                    .children(loose_reports.into_iter().map(|r| {
                        let id = r.id.clone();
                        Button::new(SharedString::from(format!("loose-report:{}", r.id)))
                            .ghost()
                            .small()
                            .start_label(Some(IconName::FileText.into()), r.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| this.open_report(&id, None, cx)))
                    }))
            })
    }

    /// The bar above a form or report opened from a page: back link and the object's name.
    fn render_crumb(&self, focus: Focus, name: String, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.pages.read(cx).current(cx).map(|p| p.name).filter(|n| !n.is_empty());
        let back = match page {
            Some(name) => format!("Back to {name}"),
            None => "Back to pages".to_string(),
        };
        let kind = if focus == Focus::Form { "Form" } else { "Report" };
        h_flex()
            .id("design-crumb")
            .gap_2()
            .items_center()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("back-to-page")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .label(back)
                    .on_click(cx.listener(|this, _, _, cx| this.back_to_page(cx))),
            )
            .child(widgets::text("design-crumb-title", format!("{kind}: {name}")))
            .when(focus == Focus::Form, |el| {
                el.child(div().flex_1())
                    .child(
                        Button::new("duplicate-form")
                            .outline()
                            .small()
                            .icon(IconName::Copy)
                            .label("Duplicate form")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.forms.read(cx).form_id.clone().unwrap_or_default();
                                this.forms.update(cx, |f, cx| f.copy_form(&id, cx));
                            })),
                    )
                    .child(
                        Button::new("delete-form")
                            .outline()
                            .small()
                            .icon(IconName::Delete)
                            .label("Delete form")
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.forms.read(cx).form_id.clone().unwrap_or_default();
                                this.forms.update(cx, |f, cx| f.remove_form(&id, cx));
                                this.back_to_page(cx);
                            })),
                    )
            })
    }
}

impl Render for DesignMode {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The shell is being updated while it builds this view, so reveals are taken here.
        self.take_reveal(cx);
        let focus = self.shown_focus(cx);
        let config = self.config(cx);
        let (title, body) = match focus {
            Focus::Page => {
                let title = self.pages.read(cx).current(cx).map_or("Pages".to_string(), |p| p.name);
                (title, self.pages.clone().into_any_element())
            }
            Focus::Navigation => ("Navigation".to_string(), self.forms.clone().into_any_element()),
            Focus::Form => {
                let id = self.forms.read(cx).form_id.clone();
                let name = config.design.forms.iter().find(|f| Some(&f.id) == id.as_ref()).map(|f| f.name.clone()).unwrap_or_default();
                let body = v_flex()
                    .size_full()
                    .child(self.render_crumb(focus, name.clone(), cx))
                    .child(div().flex_1().min_h_0().child(self.forms.clone()))
                    .into_any_element();
                (name, body)
            }
            Focus::Report => {
                let id = self.reports.read(cx).selected.clone();
                let name = config.reports.iter().find(|r| Some(&r.id) == id.as_ref()).map(|r| r.name.clone()).unwrap_or_default();
                let body = v_flex()
                    .size_full()
                    .child(self.render_crumb(focus, name.clone(), cx))
                    .child(div().flex_1().min_h_0().child(self.reports.clone()))
                    .into_any_element();
                (name, body)
            }
        };
        let sidebar = self.render_sidebar(focus, cx);
        v_flex()
            .size_full()
            .child(widgets::titlebar("Project / Design", &title, cx))
            .child(
                h_flex()
                    .id("design-mode")
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(div().flex_1().min_w_0().h_full().child(body)),
            )
    }
}
