//! The ixtable design system styleguide, built with gpui-styleguidist.
//!
//! - `cargo run -p ixtable-design --example styleguide` opens it natively.
//! - `-- render` re-renders the committed snapshots under `styleguide/`;
//!   `-- check` compares against them (CI).

use gpui_kit::assets::IconName;
use gpui_kit::base::DialogHandle;
use gpui_kit::{
    base::{h_flex, v_flex},
    prelude::FluentBuilder as _,
    *,
};
use gpui_styleguidist::{Styleguide, example};
use ixtable_design::{ColorMode, Platform, prelude::*};

fn main() {
    gpui_styleguidist::main(styleguide());
}

/// Theme names, `<platform>-<mode>`: every desktop platform in light and dark.
fn themes() -> Vec<String> {
    ixtable_design::DESKTOP
        .iter()
        .flat_map(|platform| ["light", "dark"].map(|mode| format!("{}-{mode}", slug(*platform))))
        .collect()
}

fn slug(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOS => "macos",
        Platform::Windows => "windows",
        _ => "linux",
    }
}

fn apply_theme(name: &str, cx: &mut App) {
    let (platform, mode) = name.rsplit_once('-').unwrap_or((name, "light"));
    let platform = ixtable_design::DESKTOP
        .into_iter()
        .find(|candidate| slug(*candidate) == platform)
        .unwrap_or(Platform::Linux);
    let mode = if mode == "dark" {
        ColorMode::Dark
    } else {
        ColorMode::Light
    };
    ixtable_design::theme::theme(platform, mode).apply(cx);
}

const TONES: &[(&str, Tone)] = &[
    ("neutral", Tone::Neutral),
    ("info", Tone::Info),
    ("success", Tone::Success),
    ("warning", Tone::Warning),
    ("danger", Tone::Danger),
];

#[cfg(not(target_family = "wasm"))]
fn assets() -> gpui_kit::assets::Assets {
    gpui_kit::assets::Assets
}

/// On the web, the few Lucide icons the examples use are compiled in (gpui-kit
/// would fetch them over HTTP, which a sandboxed page can't rely on).
#[cfg(target_family = "wasm")]
struct WebIcons;

#[cfg(target_family = "wasm")]
impl AssetSource for WebIcons {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        macro_rules! icons {
            ($($name:literal),*) => {
                [$((concat!("icons/", $name, ".svg"), include_bytes!(concat!("icons/", $name, ".svg")).as_slice())),*]
            };
        }
        let icons = icons!(
            "undo-2",
            "redo-2",
            "refresh-cw",
            "search",
            "plus",
            "layout-dashboard",
            "chart-pie",
            "x"
        );
        Ok(icons
            .into_iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| std::borrow::Cow::Borrowed(bytes)))
    }

    fn list(&self, _: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

#[cfg(target_family = "wasm")]
fn assets() -> WebIcons {
    WebIcons
}

fn styleguide() -> Styleguide {
    Styleguide::new("ixtable design system")
        .assets(assets())
        .setup(ixtable_design::init)
        .open_window(|options, frame, cx| Ok(gpui_kit::open_window(options, cx, frame)?.0))
        .themes(themes(), apply_theme)
        .background(|cx| cx.theme().colors.body_bg)
        .foreground(|cx| cx.theme().colors.body_color)
        .variant_limit(8)
        .describe(
            "Typography",
            "Six roles. One Title per screen; Section groups what sits under it; Caption for \
             hints and counts. Never set a raw size.",
        )
        .describe(
            "ActionButton",
            "The one button. Primary is filled teal and appears at most once per screen or \
             dialog. A disabled action always says why underneath.",
        )
        .describe(
            "PageHeader",
            "Where am I, what is this, what can I do. Breadcrumb for drill-ins, one sentence of \
             state, actions on the right with the primary last.",
        )
        .describe(
            "Callout",
            "An inline message about this screen. The title is the answer; technical text goes \
             in details, never the title.",
        )
        .describe(
            "Toast",
            "The result of an action, bottom left over the sidebar, gone in a few seconds. Past tense, names the \
             thing: \"Saved Orders\".",
        )
        .describe(
            "StatusBadge",
            "A state in one or two words. Tone carries meaning, never decoration.",
        )
        .describe(
            "Field",
            "Label above, \"Optional\" instead of a required star, hint below. An error \
             replaces the hint and says how to fix it.",
        )
        .describe(
            "Section",
            "A titled group. When collapsible, its summary answers the question while closed.",
        )
        .describe("KeyValue", "Facts about one thing.")
        .describe(
            "DataTable",
            "A read-only list of records. Label headers, numbers right-aligned, an empty state \
             instead of an empty grid.",
        )
        .describe(
            "EmptyState",
            "What goes here, and the one way to add the first one.",
        )
        .describe(
            "Tabs",
            "Views of the same thing. Inactive tabs are muted text, never links or buttons; the \
             accent marks the active one.",
        )
        .examples(foundations())
        .examples(actions())
        .examples(feedback())
        .describe(
            "AppShell",
            "The frame every screen sits in. Sidebar, Ribbon and PageHeader together pass \
             Krug's trunk test: which document, which page, its sections, my options, where I am.",
        )
        .describe(
            "Ribbon",
            "Tabs over labelled command groups, the same on every page. Commands are short verbs \
             under a noun group label; a disabled command says why in its tooltip; Run App is \
             the one primary.",
        )
        .describe(
            "Sidebar",
            "The document's objects by kind, with counts. The open object is tinted; each group \
             ends with the way to add one.",
        )
        .describe(
            "Dialog",
            "One decision. The title asks the question; the footer holds Cancel and exactly one \
             primary, or a filled danger confirm.",
        )
        .describe(
            "RecordForm",
            "One column of fields and one Save. After a failed save, a callout counts the \
             problems and each field shows its own fix.",
        )
        .examples(content())
        .examples(composites())
}

fn foundations() -> Vec<gpui_styleguidist::Example> {
    vec![
        example!("typography/scale", |_, _, _| {
            v_flex().gap_3().children(Type::ALL.map(|ty| {
                h_flex()
                    .gap_4()
                    .items_baseline()
                    .child(div().w(px(80.)).ty(Type::Caption).child(ty.name()))
                    .child(div().ty(ty).child("Orders waiting to ship"))
            }))
        })
        .component("Typography")
        .title("Type scale"),
    ]
}

fn actions() -> Vec<gpui_styleguidist::Example> {
    vec![
        example!("action/playground", |knobs, _, _| {
            let disabled = knobs.bool("disabled", false);
            ActionButton::new(
                "action",
                knobs.text_samples("label", &["Create table", "Import records…"]),
            )
            .emphasis(knobs.pick(
                "emphasis",
                &[
                    ("primary", Emphasis::Primary),
                    ("secondary", Emphasis::Secondary),
                    ("quiet", Emphasis::Quiet),
                    ("danger", Emphasis::Danger),
                ],
                "primary",
            ))
            .when(disabled, |this| {
                this.disabled_because("Choose a file first")
            })
        })
        .component("ActionButton")
        .title("Playground"),
        example!("action/hierarchy", |_, _, _| {
            v_flex()
                .gap_4()
                .child(
                    h_flex()
                        .gap_2()
                        .child(ActionButton::quiet("help", "What's a relationship?"))
                        .child(ActionButton::new("cancel", "Cancel"))
                        .child(ActionButton::primary("create", "Create relationship")),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(ActionButton::new("keep", "Keep table"))
                        .child(ActionButton::danger("delete", "Delete table").confirm()),
                )
                .child(ActionButton::danger("row-delete", "Delete record").small())
        })
        .component("ActionButton")
        .title("One primary per surface")
        .description("A form footer, a danger confirm, and a row action."),
        example!("action/looks", |_, _, _| {
            v_flex().gap_3().children(ButtonLook::ALL.map(|look| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(64.)).ty(Type::Caption).child(look.name()))
                    .child(ActionButton::new(("cancel", look as usize), "Cancel").look(look))
                    .child(ActionButton::danger(("delete", look as usize), "Delete").look(look))
                    .child(ActionButton::primary(("save", look as usize), "Save"))
            }))
        })
        .component("ActionButton")
        .title("Looks")
        .description("Outline everywhere; soft in dense rows where borders add noise."),
        example!("page-header/playground", |knobs, _, _| {
            div().w(px(640.)).child(
                PageHeader::new("header", "Orders")
                    .when(knobs.bool("breadcrumb", true), |this| {
                        this.breadcrumb(
                            gpui_bootstrap::breadcrumb::Breadcrumb::new("crumbs")
                                .item("Northwind", |_, _, _| {})
                                .active("Orders"),
                        )
                    })
                    .description(knobs.text_samples(
                        "description",
                        &["830 orders. 3 are waiting to ship.", "No orders yet."],
                    ))
                    .action(ActionButton::new("import", "Import records…"))
                    .action(ActionButton::primary("new", "New order")),
            )
        })
        .component("PageHeader")
        .title("Playground"),
        example!("tabs/basic", |knobs, _, _| {
            let style = knobs.pick(
                "style",
                &[
                    ("underline", TabStyle::Underline),
                    ("pills", TabStyle::Pills),
                    ("segmented", TabStyle::Segmented),
                ],
                "underline",
            );
            let counts = knobs.bool("counts", false);
            Tabs::new("tabs", ["Records", "Fields", "Relationships"])
                .style(style)
                .when(knobs.bool("vertical", false), |this| this.vertical())
                .when(counts, |this| this.counts([Some(830), Some(14), None]))
                .active(knobs.pick(
                    "active",
                    &[("records", 0), ("fields", 1), ("relationships", 2)],
                    "records",
                ))
        })
        .component("Tabs")
        .title("Playground"),
        example!("tabs/styles", |_, _, _| {
            v_flex()
                .gap_4()
                .child(Tabs::new("page", ["Records", "Fields", "Relationships"]))
                .child(
                    Tabs::new("filter", ["All", "Open", "Shipped", "Late"])
                        .style(TabStyle::Pills)
                        .counts([Some(830), Some(12), Some(815), Some(3)])
                        .active(1),
                )
                .child(Tabs::new("mode", ["Design", "Preview"]).style(TabStyle::Segmented))
        })
        .component("Tabs")
        .title("Styles")
        .description(
            "Underline for a page's views, pills for filters (with counts), segmented for two to \
             four modes of one panel.",
        ),
        example!("tabs/vertical", |_, _, _| {
            h_flex()
                .gap_6()
                .items_start()
                .child(
                    div().w(px(200.)).child(
                        Tabs::new(
                            "settings",
                            ["General", "Appearance", "Users and roles", "Backups"],
                        )
                        .vertical()
                        .active(1),
                    ),
                )
                .child(
                    div().w(px(200.)).child(
                        Tabs::new("queues", ["Inbox", "Assigned to me", "Done"])
                            .style(TabStyle::Pills)
                            .vertical()
                            .counts([Some(24), Some(5), None]),
                    ),
                )
        })
        .component("Tabs")
        .title("Vertical")
        .description("Settings sections and long lists of views stack down the left."),
    ]
}

fn feedback() -> Vec<gpui_styleguidist::Example> {
    vec![
        example!("callout/playground", |knobs, _, _| {
            div().w(px(480.)).child(
                Callout::new(
                    "callout",
                    knobs.pick("tone", TONES, "warning"),
                    knobs.text("title", "3 orders can't be shipped"),
                )
                .body(knobs.text(
                    "body",
                    "Their customers have no address. Add one, then try again.",
                ))
                .when(knobs.bool("details", false), |this| {
                    this.details("NOT NULL constraint failed: orders.ship_address")
                })
                .when(knobs.bool("action", true), |this| {
                    this.action(ActionButton::new("fix", "Show customers").small())
                }),
            )
        })
        .component("Callout")
        .title("Playground"),
        example!("toast/tones", |_, _, _| {
            v_flex()
                .gap_2()
                .child(Toast::new("saved", Tone::Success, "Saved Orders"))
                .child(
                    Toast::new("imported", Tone::Warning, "Imported 120 records")
                        .detail("4 rows were skipped. See the import report."),
                )
                .child(
                    Toast::new("failed", Tone::Danger, "Couldn't publish")
                        .detail("You're offline.")
                        .on_dismiss(|_, _, _| {}),
                )
        })
        .component("Toast")
        .title("Tones"),
        example!("status-badge/tones", |_, _, _| {
            h_flex().gap_2().children([
                StatusBadge::new(Tone::Neutral, "Draft"),
                StatusBadge::new(Tone::Info, "Scheduled"),
                StatusBadge::new(Tone::Success, "Shipped"),
                StatusBadge::new(Tone::Warning, "Waiting to ship"),
                StatusBadge::new(Tone::Danger, "Failed"),
            ])
        })
        .component("StatusBadge")
        .title("Tones"),
    ]
}

fn content() -> Vec<gpui_styleguidist::Example> {
    vec![
        example!("field/states", |knobs, _, _| {
            let state = knobs.pick(
                "state",
                &[("hint", 0), ("error", 1), ("optional", 2)],
                "hint",
            );
            let field = Field::new("field", "Ship date").control(
                div()
                    .w(px(280.))
                    .h(px(30.))
                    .border_1()
                    .rounded_md()
                    .border_color(gpui_kit::rgb(0xCED4DA)),
            );
            match state {
                1 => field.error(Some("Enter a date like 2026-10-22")),
                2 => field.optional().hint("Leave empty until the order ships."),
                _ => field.hint("The day the order leaves the warehouse."),
            }
        })
        .component("Field")
        .title("States"),
        example!("section/playground", |knobs, _, _| {
            div().w(px(520.)).child(
                Section::new("section", "Relationships")
                    .summary("3 relationships")
                    .when(knobs.bool("collapsible", true), |this| {
                        this.collapsible(knobs.bool("open", true), |_, _, _| {})
                    })
                    .child(
                        KeyValue::new("kv")
                            .text("Orders → Customers", "Each order has one customer")
                            .text("Orders → Employees", "Each order has one employee"),
                    ),
            )
        })
        .component("Section")
        .title("Playground"),
        example!("key-value/basic", |_, _, _| {
            div().w(px(420.)).child(
                KeyValue::new("kv")
                    .text("File", "northwind.ixt")
                    .text("Last saved", "Today at 14:02")
                    .row("Status", StatusBadge::new(Tone::Success, "Published")),
            )
        })
        .component("KeyValue")
        .title("Basic"),
        example!("data-table/playground", |knobs, _, _| {
            let columns = [
                Column::new("Order").width(70.),
                Column::new("Customer"),
                Column::new("Status").width(140.),
                Column::number("Total").width(90.),
            ];
            let table = DataTable::new("orders", columns)
                .empty(
                    EmptyState::new("no-orders", "No orders yet")
                        .body("Orders you create or import appear here.")
                        .action(ActionButton::primary("new-order", "New order")),
                )
                .selected(Some(1))
                .on_select(|_, _, _, _| {})
                .footer("4 of 830 orders");
            let rows: [(&str, &str, Tone, &str, &str); 4] = [
                (
                    "10248",
                    "Vins et alcools Chevalier",
                    Tone::Warning,
                    "Waiting to ship",
                    "$440.00",
                ),
                (
                    "10249",
                    "Toms Spezialitäten",
                    Tone::Success,
                    "Shipped",
                    "$1,863.40",
                ),
                (
                    "10250",
                    "Hanari Carnes",
                    Tone::Warning,
                    "Waiting to ship",
                    "$1,552.60",
                ),
                (
                    "10251",
                    "Victuailles en stock",
                    Tone::Neutral,
                    "Draft",
                    "$654.06",
                ),
            ];
            let table = if knobs.bool("empty", false) {
                table
            } else {
                rows.into_iter()
                    .fold(table, |table, (order, customer, tone, status, total)| {
                        table.row([
                            order.into_any_element(),
                            customer.into_any_element(),
                            StatusBadge::new(tone, status).into_any_element(),
                            total.into_any_element(),
                        ])
                    })
            };
            div().w(px(600.)).child(table)
        })
        .component("DataTable")
        .title("Playground"),
        example!("empty-state/playground", |knobs, _, _| {
            EmptyState::new("empty", knobs.text("title", "No tables yet"))
                .body(knobs.text(
                    "body",
                    "Tables hold your records. Start with one, or import a spreadsheet.",
                ))
                .action(ActionButton::primary("create", "Create table"))
        })
        .component("EmptyState")
        .title("Playground"),
    ]
}

fn sample_sidebar(selected: &'static str) -> Sidebar {
    Sidebar::new("sidebar", "Northwind")
        .mark(div().size(px(18.)).rounded_md().bg(gpui_kit::rgb(0x0DB9B6)))
        .group(
            SidebarGroup::new("tables", "Tables")
                .item(SidebarItem::new("customers", "Customers").count(91))
                .item(SidebarItem::new("orders", "Orders").count(830))
                .item(SidebarItem::new("products", "Products").count(77))
                .add("New table…", |_, _, _| {}),
        )
        .group(
            SidebarGroup::new("queries", "Queries")
                .item(SidebarItem::new("late-orders", "Late orders").count(3))
                .add("New query…", |_, _, _| {}),
        )
        .group(
            SidebarGroup::new("pages", "Pages")
                .item(SidebarItem::new("order-entry", "Order entry"))
                .item(SidebarItem::new("sales-report", "Sales by month"))
                .add("New page…", |_, _, _| {}),
        )
        .selected(selected)
}

fn sample_ribbon(tab: usize) -> Ribbon {
    let ribbon = Ribbon::new(
        "ribbon",
        ["Home", "Create", "External data", "Database tools"],
    )
    .active(tab)
    .primary(ActionButton::new("run-app", "Run App"));
    match tab {
        1 => ribbon
            .group(
                RibbonGroup::new("tables", "Tables")
                    .command(RibbonCommand::new("new-table", IconName::Plus, "Table"))
                    .command(RibbonCommand::new("new-query", IconName::Search, "Query")),
            )
            .group(
                RibbonGroup::new("pages", "Pages")
                    .command(RibbonCommand::new(
                        "new-form",
                        IconName::LayoutDashboard,
                        "Form",
                    ))
                    .command(RibbonCommand::new(
                        "new-report",
                        IconName::ChartPie,
                        "Report",
                    )),
            ),
        _ => ribbon
            .group(
                RibbonGroup::new("undo", "Undo")
                    .command(RibbonCommand::new("undo", IconName::Undo2, "Undo"))
                    .command(
                        RibbonCommand::new("redo", IconName::Redo2, "Redo")
                            .disabled_because("Nothing to redo"),
                    ),
            )
            .group(
                RibbonGroup::new("records", "Records")
                    .command(RibbonCommand::new(
                        "refresh",
                        IconName::RefreshCw,
                        "Refresh",
                    ))
                    .command(RibbonCommand::new("find", IconName::Search, "Find")),
            ),
    }
}

fn orders_table() -> DataTable {
    let columns = [
        Column::new("Order").width(70.),
        Column::new("Customer"),
        Column::new("Status").width(150.),
        Column::number("Total").width(100.),
    ];
    let rows: [(&str, &str, Tone, &str, &str); 4] = [
        (
            "10248",
            "Vins et alcools Chevalier",
            Tone::Warning,
            "Waiting to ship",
            "$440.00",
        ),
        (
            "10249",
            "Toms Spezialitäten",
            Tone::Success,
            "Shipped",
            "$1,863.40",
        ),
        (
            "10250",
            "Hanari Carnes",
            Tone::Warning,
            "Waiting to ship",
            "$1,552.60",
        ),
        (
            "10251",
            "Victuailles en stock",
            Tone::Neutral,
            "Draft",
            "$654.06",
        ),
    ];
    rows.into_iter().fold(
        DataTable::new("orders", columns)
            .selected(Some(1))
            .footer("4 of 830 orders"),
        |table, (order, customer, tone, status, total)| {
            table.row([
                order.into_any_element(),
                customer.into_any_element(),
                StatusBadge::new(tone, status).into_any_element(),
                total.into_any_element(),
            ])
        },
    )
}

/// An open [`Dialog`] over a page.
struct DialogDemo {
    handle: DialogHandle,
    destructive: bool,
}

impl Render for DialogDemo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dialog = if self.destructive {
            Dialog::new(&self.handle, "Delete the Orders table?")
                .size(DialogSize::Small)
                .cancel_label("Keep table")
                .primary(ActionButton::danger("delete", "Delete table"))
                .child("830 records and 2 relationships go with it. You can undo this until you close the document.")
        } else {
            Dialog::new(&self.handle, "New relationship")
                .primary(ActionButton::new("create", "Create relationship"))
                .child(
                    Field::new("from", "Each order belongs to")
                        .control(fake_input("Customers"))
                        .hint("Each order links to one customer record."),
                )
        };
        // The dialog is a window overlay, so the preview opens it from a trigger
        // instead of covering the whole styleguide on load.
        let handle = self.handle.clone();
        let trigger = if self.destructive {
            ActionButton::danger("open-dialog", "Delete table…")
        } else {
            ActionButton::new("open-dialog", "New relationship…")
        };
        div()
            .size_full()
            .p_4()
            .child(trigger.on_click(move |_, window, cx| handle.open(window, cx)))
            .child(dialog)
    }
}

fn fake_input(value: &'static str) -> impl IntoElement {
    div()
        .w(px(300.))
        .px_2()
        .py_1()
        .border_1()
        .rounded_md()
        .border_color(gpui_kit::rgb(0xC9CED4))
        .child(value)
}

fn composites() -> Vec<gpui_styleguidist::Example> {
    vec![
        example!("ribbon/studio", |knobs, _, _| {
            div().w(px(820.)).child(sample_ribbon(knobs.pick(
                "tab",
                &[("home", 0), ("create", 1)],
                "home",
            )))
        })
        .component("Ribbon")
        .title("Studio ribbon"),
        example!("sidebar/document", |knobs, _, _| {
            div().h(px(440.)).child(sample_sidebar(knobs.pick(
                "selected",
                &[("orders", "orders"), ("late-orders", "late-orders")],
                "orders",
            )))
        })
        .component("Sidebar")
        .title("Document objects"),
        example!(view "dialog/confirm", |knobs, _, cx| {
            cx.new(|_| DialogDemo {
                handle: DialogHandle::new(knobs.bool("open", false)),
                destructive: knobs.pick("kind", &[("delete", true), ("form", false)], "delete"),
            })
        })
        .component("Dialog")
        .title("Confirm and form")
        .description("Press the button to open it; Escape or Cancel closes it. The open knob shows it open.")
        .size(720., 380.),
        example!("record-form/order", |knobs, _, _| {
            let failed = knobs.bool("failed save", false);
            RecordForm::new("order-form")
                .problems(if failed { 1 } else { 0 })
                .field(Field::new("customer", "Customer").control(fake_input("Hanari Carnes")))
                .field(
                    Field::new("ship-date", "Ship date")
                        .control(fake_input(if failed { "next tuesday" } else { "2026-10-28" }))
                        .hint("The day the order leaves the warehouse.")
                        .error(failed.then_some("Enter a date like 2026-10-22")),
                )
                .field(
                    Field::new("notes", "Notes")
                        .optional()
                        .control(fake_input("")),
                )
                .primary(ActionButton::new("save", "Save order"))
                .action(ActionButton::quiet("discard", "Discard changes"))
        })
        .component("RecordForm")
        .title("Order form"),
        example!("app-shell/studio", |knobs, _, _| {
            AppShell::new("shell")
                .sidebar(sample_sidebar("orders"))
                .ribbon(sample_ribbon(0))
                .header(
                    PageHeader::new("header", "Orders")
                        .description("830 orders. 3 are waiting to ship.")
                        .action(ActionButton::new("import", "Import records…"))
                        .primary(ActionButton::new("new", "New order")),
                )
                .child(orders_table())
                .when(knobs.bool("toast", true), |this| {
                    this.toast(Toast::new("saved", Tone::Success, "Saved Orders"))
                })
        })
        .component("AppShell")
        .title("Studio: Orders")
        .description("The trunk test: document (sidebar title), page (Title), sections (ribbon tabs and sidebar groups), options (ribbon and header actions), you are here (tinted sidebar item, underlined tab).")
        .size(1180., 560.),
    ]
}
