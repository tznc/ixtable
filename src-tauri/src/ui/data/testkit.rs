//! Helpers shared by the Data mode tests: reaching the views behind the harness, and
//! driving the dropdown menus that selects and column menus open.

use super::{
    sheet::{find::CellAt, grid::Datasheet},
    workbench::DataMode,
};
use crate::ui::testing::Harness;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::Entity;

/// The Data mode view of the open document.
pub fn data_mode(h: &mut Harness) -> Entity<DataMode> {
    let view = h.read_shell(|s, _| s.mode_view()).expect("a mode view");
    view.downcast().expect("Data mode is showing")
}

/// The datasheet of the selected table.
pub fn sheet(h: &mut Harness) -> Entity<Datasheet> {
    let mode = data_mode(h);
    h.cx.read(|cx| mode.read(cx).sheet().cloned()).expect("a datasheet")
}

/// Pastes clipboard text into the datasheet at a cell, as the cell's paste handler does.
pub fn paste_at(h: &mut Harness, row: usize, column: usize, text: &str) -> bool {
    let sheet = sheet(h);
    let text = text.to_string();
    let pasted = h.update(|window, cx| {
        sheet.update(cx, |s, cx| s.paste(&text, CellAt { row, column }, window, cx))
    });
    h.settle();
    pasted
}

/// The labels of the open dropdown menu's items.
pub fn menu_labels(h: &mut Harness) -> Vec<String> {
    h.update(|window, _| {
        let menu = window.within("popup-menu");
        (0usize..)
            .map_while(|ix| menu.try_find(ix))
            .map(|item| item.label().unwrap_or_default().to_string())
            .collect()
    })
}

/// Opens the dropdown `id` and chooses the item labelled `label`.
pub fn choose(h: &mut Harness, id: &str, label: &str) {
    h.click(id.to_string());
    let labels = menu_labels(h);
    let index = labels
        .iter()
        .position(|l| l == label)
        .unwrap_or_else(|| panic!("no menu item {label:?} in {labels:?}"));
    h.update(|window, cx| window.within("popup-menu").click(index, cx));
    h.settle();
}

/// The accessibility label of `id` inside the scope `scope`.
pub fn label_in(h: &mut Harness, scope: &str, id: &str) -> String {
    let (scope, id) = (scope.to_string(), id.to_string());
    h.update(|window, _| window.within(scope).find(id).label().unwrap_or_default().to_string())
}

/// Whether `id` exists inside the scope `scope`.
pub fn exists_in(h: &mut Harness, scope: &str, id: &str) -> bool {
    let (scope, id) = (scope.to_string(), id.to_string());
    h.update(|window, _| window.within(scope).try_find(id).is_some())
}

/// The table schema as the backend reports it.
pub fn inspect(h: &Harness, table: &str) -> crate::data::TableSchema {
    crate::inspect_table(h.label.clone(), table.into(), None).expect("inspect table")
}

/// Scrolls the observed container `container` until `id` (inside `scope`, if given) is
/// visible, as a user would with the wheel.
pub fn reveal(h: &mut Harness, container: &str, scope: Option<&str>, id: &str) {
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
pub fn auto_reveal(h: &mut Harness, id: &str) {
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

impl Harness<'_> {
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
pub fn click_designer(h: &mut Harness, id: &str) {
    reveal(h, "table-designer", None, id);
    h.click(id.to_string());
}

/// Reveals and clicks `id` inside the designer section `scope`.
pub fn click_designer_in(h: &mut Harness, scope: &str, id: &str) {
    reveal(h, "table-designer", Some(scope), id);
    h.click_in(scope.to_string(), id.to_string());
}
