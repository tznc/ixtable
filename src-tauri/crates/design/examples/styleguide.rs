//! The ixtable design system styleguide, built with gpui-styleguidist.
//!
//! - `cargo run -p ixtable-design --example styleguide` opens it natively.
//! - `-- render` re-renders the committed snapshots under `styleguide/`;
//!   `-- check` compares against them (CI).

use gpui_kit::{
    base::{h_flex, v_flex},
    prelude::FluentBuilder as _,
    *,
};
use gpui_styleguidist::{Styleguide, example};
use ixtable_design::{ColorMode, prelude::*};

fn main() {
    gpui_styleguidist::main(styleguide());
}

fn apply_theme(name: &str, cx: &mut App) {
    let mode = if name == "dark" {
        ColorMode::Dark
    } else {
        ColorMode::Light
    };
    ixtable_design::change_mode(mode, cx);
}

const TONES: &[(&str, Tone)] = &[
    ("neutral", Tone::Neutral),
    ("info", Tone::Info),
    ("success", Tone::Success),
    ("warning", Tone::Warning),
    ("danger", Tone::Danger),
];

fn styleguide() -> Styleguide {
    Styleguide::new("ixtable design system")
        .assets(gpui_kit::assets::Assets)
        .setup(ixtable_design::init)
        .open_window(|options, frame, cx| Ok(gpui_kit::open_window(options, cx, frame)?.0))
        .themes(vec!["light".to_string(), "dark".to_string()], apply_theme)
        .background(|cx| cx.theme().colors.body_bg)
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
            "The result of an action, top right, gone in a few seconds. Past tense, names the \
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
            "Views of the same thing. Underlined, never button-shaped.",
        )
        .examples(foundations())
        .examples(actions())
        .examples(feedback())
        .examples(content())
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
            Tabs::new("tabs", ["Records", "Fields", "Relationships"]).active(knobs.pick(
                "active",
                &[("records", 0), ("fields", 1), ("relationships", 2)],
                "records",
            ))
        })
        .component("Tabs")
        .title("Tabs"),
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
