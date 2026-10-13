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
