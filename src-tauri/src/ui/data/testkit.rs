//! Helpers shared by the Data mode tests: reaching the views behind the harness, and
//! driving the dropdown menus that selects and column menus open.

use super::{
    sheet::{find::CellAt, grid::Datasheet},
    workbench::DataMode,
};
use crate::ui::testing::{Harness, UiContext};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::Entity;

/// The Data mode view of the open document.
pub fn data_mode<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<DataMode> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("a mode view");
    view.downcast().expect("Data mode is showing")
}

/// The datasheet of the selected table.
pub fn sheet<C: UiContext>(h: &mut Harness<'_, C>) -> Entity<Datasheet> {
    let mode = data_mode(h);
    h.cx.read(|cx| mode.read(cx).sheet().cloned()).expect("a datasheet")
}

/// Pastes clipboard text into the datasheet at a cell, as the cell's paste handler does.
pub fn paste_at<C: UiContext>(h: &mut Harness<'_, C>, row: usize, column: usize, text: &str) -> bool {
    let sheet = sheet(h);
    let text = text.to_string();
    let pasted = h.update(|window, cx| {
        sheet.update(cx, |s, cx| s.paste(&text, CellAt { row, column }, window, cx))
    });
    h.settle();
    pasted
}

/// The labels of the open dropdown menu's items.
pub fn menu_labels<C: UiContext>(h: &mut Harness<'_, C>) -> Vec<String> {
    h.update(|window, _| {
        let menu = window.within("popup-menu");
        (0usize..)
            .map_while(|ix| menu.try_find(ix))
            .map(|item| item.label().unwrap_or_default().to_string())
            .collect()
    })
}

/// Opens the dropdown `id` and chooses the item labelled `label`.
pub fn choose<C: UiContext>(h: &mut Harness<'_, C>, id: &str, label: &str) {
    h.click(id.to_string());
    choose_open(h, label);
}

/// Chooses the item labelled `label` in the dropdown menu that is already open.
pub fn choose_open<C: UiContext>(h: &mut Harness<'_, C>, label: &str) {
    let labels = menu_labels(h);
    let index = labels
        .iter()
        .position(|l| l == label)
        .unwrap_or_else(|| panic!("no menu item {label:?} in {labels:?}"));
    h.update(|window, cx| window.within("popup-menu").click(index, cx));
    h.settle();
}

/// The accessibility label of `id` inside the scope `scope`.
pub fn label_in<C: UiContext>(h: &mut Harness<'_, C>, scope: &str, id: &str) -> String {
    let (scope, id) = (scope.to_string(), id.to_string());
    h.update(|window, _| window.within(scope).find(id).label().unwrap_or_default().to_string())
}

/// Whether `id` exists inside the scope `scope`.
pub fn exists_in<C: UiContext>(h: &mut Harness<'_, C>, scope: &str, id: &str) -> bool {
    let (scope, id) = (scope.to_string(), id.to_string());
    h.update(|window, _| window.within(scope).try_find(id).is_some())
}

/// The table schema as the backend reports it.
pub fn inspect<C: UiContext>(h: &Harness<'_, C>, table: &str) -> crate::data::TableSchema {
    crate::inspect_table(h.label.clone(), table.into(), None).expect("inspect table")
}

/// Scrolls the observed container `container` until `id` (inside `scope`, if given) is
/// visible, as a user would with the wheel.
pub fn reveal<C: UiContext>(h: &mut Harness<'_, C>, container: &str, scope: Option<&str>, id: &str) {
    use gpui_kit::{point, px, ScrollDelta};
    for _ in 0..60 {
        let (container, scope, id) = (container.to_string(), scope.map(str::to_string), id.to_string());
        let done = h.update(move |window, cx| {
            window.render_frame(cx);
            let target = match &scope {
                Some(scope) => window.within(scope.clone()).find(id.clone()),
                None => window.find(id.clone()),
            };
            let view = window.find(container.clone()).bounds();
            let centre = target.bounds().center();
            let inside = centre.y >= view.origin.y && centre.y <= view.origin.y + view.size.height;
            if target.visible() && inside && centre.y < window.viewport_size().height {
                return true;
            }
            let below = target.bounds().origin.y >= view.origin.y;
            let step = if below { -240. } else { 240. };
            window.scroll(container, ScrollDelta::Pixels(point(px(0.), px(step))), cx);
            false
        });
        if done {
            return;
        }
    }
    panic!("{id:?} stayed out of view in {container:?}");
}

/// Scrolls the form's scroll area (designer or create-table form) to `id`, if it is
/// off screen.
pub fn auto_reveal<C: UiContext>(h: &mut Harness<'_, C>, id: &str) {
    for container in ["table-designer", "create-table-body", "import-dialog"] {
        let (c, i) = (container.to_string(), id.to_string());
        let off_screen = h.update(move |window, _| {
            window.try_find(c).is_some()
                && window.try_find(i).is_some_and(|t| !t.visible() || t.bounds().center().y > window.viewport_size().height)
        });
        if off_screen {
            reveal(h, container, None, id);
            return;
        }
    }
}

impl<C: UiContext> Harness<'_, C> {
    /// Clicks `id`, scrolling it into view first.
    pub fn go(&mut self, id: &str) {
        auto_reveal(self, id);
        self.click(id.to_string());
    }

    /// Replaces the text of the input `id`, scrolling it into view first.
    pub fn enter(&mut self, id: &str, text: &str) {
        auto_reveal(self, id);
        self.fill(id.to_string(), text);
    }

    /// Appends text to the input `id`, scrolling it into view first.
    pub fn append(&mut self, id: &str, text: &str) {
        auto_reveal(self, id);
        self.type_into(id.to_string(), text);
    }

    /// Chooses `label` in the select `id`, scrolling it into view first.
    pub fn pick(&mut self, id: &str, label: &str) {
        auto_reveal(self, id);
        choose(self, id, label);
    }
}

/// Reveals `id` in the table designer's scroll area, then clicks it.
pub fn click_designer<C: UiContext>(h: &mut Harness<'_, C>, id: &str) {
    reveal(h, "table-designer", None, id);
    h.click(id.to_string());
}

/// Reveals and clicks `id` inside the designer section `scope`.
pub fn click_designer_in<C: UiContext>(h: &mut Harness<'_, C>, scope: &str, id: &str) {
    reveal(h, "table-designer", Some(scope), id);
    h.click_in(scope.to_string(), id.to_string());
}

/// Creates a table from JSON column and foreign key lists (no UI announcement).
pub fn make_table<C: UiContext>(h: &Harness<'_, C>, name: &str, columns: serde_json::Value, foreign_keys: serde_json::Value) {
    let spec = serde_json::json!({ "name": name, "columns": columns, "foreignKeys": foreign_keys });
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).expect("table spec"))
        .expect("create table");
}

/// Inserts a record through the backend (no announcement).
pub fn put<C: UiContext>(h: &Harness<'_, C>, table: &str, record: &[(&str, crate::data::DataValue)]) {
    let values = record
        .iter()
        .map(|(column, value)| crate::data::NamedValue { column: column.to_string(), value: value.clone() })
        .collect();
    crate::recordstore::commands::insert_row(h.label.clone(), table.into(), values, None).expect("insert row");
}

fn text(value: &str) -> crate::data::DataValue {
    crate::data::DataValue::Text(value.into())
}

fn int(value: i64) -> crate::data::DataValue {
    crate::data::DataValue::Integer(value)
}

fn real(value: f64) -> crate::data::DataValue {
    crate::data::DataValue::Real(value)
}

fn fk(column: &str, table: &str, target: &str) -> serde_json::Value {
    serde_json::json!({ "columns": [column], "targetTable": table, "targetColumns": [target] })
}

/// The sample sales database of the screenshot specs: customers, orders and order_items.
pub fn seed_sales<C: UiContext>(h: &mut Harness<'_, C>) {
    use serde_json::json;
    let id = json!({ "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1 });
    make_table(
        h,
        "customers",
        json!([id, { "name": "name", "declaredType": "TEXT", "nullable": false }, { "name": "city", "declaredType": "TEXT" }]),
        json!([]),
    );
    make_table(
        h,
        "orders",
        json!([
            id,
            { "name": "customer_id", "declaredType": "INTEGER", "nullable": false },
            { "name": "status", "declaredType": "TEXT" },
            { "name": "amount", "declaredType": "REAL" },
            { "name": "note", "declaredType": "TEXT" },
        ]),
        json!([fk("customer_id", "customers", "id")]),
    );
    make_table(
        h,
        "order_items",
        json!([
            id,
            { "name": "order_id", "declaredType": "INTEGER" },
            { "name": "product", "declaredType": "TEXT" },
            { "name": "quantity", "declaredType": "INTEGER" },
        ]),
        json!([fk("order_id", "orders", "id")]),
    );
    for (i, name, city) in [(1, "Northstar Goods", "Leeds"), (2, "Juniper Supply", "Bristol"), (3, "Harbor & Pine", "York")] {
        put(h, "customers", &[("id", int(i)), ("name", text(name)), ("city", text(city))]);
    }
    for (i, customer, status, amount, note) in [
        (1, 1, "open", 120.0, Some("Gift wrap")),
        (2, 1, "shipped", 340.0, None),
        (3, 2, "open", 75.5, None),
        (4, 3, "shipped", 410.0, Some("Fragile")),
        (5, 2, "cancelled", 20.0, None),
    ] {
        let note = note.map_or(crate::data::DataValue::Null, text);
        put(h, "orders", &[("id", int(i)), ("customer_id", int(customer)), ("status", text(status)), ("amount", real(amount)), ("note", note)]);
    }
    for (i, order, product, quantity) in [(1, 1, "Ceramic pour-over set", 2), (2, 1, "Oak serving board", 1), (3, 2, "Linen napkins", 6)] {
        put(h, "order_items", &[("id", int(i)), ("order_id", int(order)), ("product", text(product)), ("quantity", int(quantity))]);
    }
    h.database_changed();
}

/// The four-table shop database of the `app-qa-03`/`app-qa-04` captures.
pub fn seed_shop<C: UiContext>(h: &mut Harness<'_, C>) {
    use serde_json::json;
    let col = |name: &str, kind: &str, pk: u32| {
        json!({ "name": name, "declaredType": kind, "nullable": pk == 0, "primaryKeyPosition": pk })
    };
    make_table(h, "Customers", json!([col("Customer ID", "TEXT", 1), col("Company", "TEXT", 0), col("Contact Name", "TEXT", 0)]), json!([]));
    make_table(h, "Products", json!([col("Product ID", "TEXT", 1), col("Product Name", "TEXT", 0), col("Price", "REAL", 0)]), json!([]));
    make_table(
        h,
        "Orders",
        json!([col("Order ID", "TEXT", 1), col("Customer ID", "TEXT", 0), col("Order Date", "TEXT", 0)]),
        json!([fk("Customer ID", "Customers", "Customer ID")]),
    );
    make_table(
        h,
        "Order Items",
        json!([col("Item ID", "INTEGER", 1), col("Order ID", "TEXT", 0), col("Product ID", "TEXT", 0), col("Quantity", "INTEGER", 0)]),
        json!([fk("Order ID", "Orders", "Order ID"), fk("Product ID", "Products", "Product ID")]),
    );
    put(h, "Customers", &[("Customer ID", text("CUST-001")), ("Company", text("Northstar Goods")), ("Contact Name", text("Avery Chen"))]);
    put(h, "Customers", &[("Customer ID", text("CUST-002")), ("Company", text("Juniper Supply")), ("Contact Name", text("Mina Patel"))]);
    put(h, "Products", &[("Product ID", text("PROD-101")), ("Product Name", text("Ceramic Pour-over Set")), ("Price", real(84.0))]);
    put(h, "Products", &[("Product ID", text("PROD-102")), ("Product Name", text("Oak Serving Board")), ("Price", real(56.0))]);
    put(h, "Orders", &[("Order ID", text("10482")), ("Customer ID", text("CUST-001")), ("Order Date", text("14 Aug 2026"))]);
    put(h, "Orders", &[("Order ID", text("10481")), ("Customer ID", text("CUST-002")), ("Order Date", text("13 Aug 2026"))]);
    put(h, "Order Items", &[("Item ID", int(1)), ("Order ID", text("10482")), ("Product ID", text("PROD-101")), ("Quantity", int(2))]);
    put(h, "Order Items", &[("Item ID", int(2)), ("Order ID", text("10481")), ("Product ID", text("PROD-102")), ("Quantity", int(1))]);
    h.database_changed();
}
