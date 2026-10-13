//! Document-wide events. Each open document has one `DocBus`; views subscribe to it
//! instead of the browser events the React UI used (`ixtable:database-changed`,
//! `ixtable:records-changed`).

use gpui_kit::*;

#[derive(Clone, Debug, PartialEq)]
pub enum DocEvent {
    /// Tables, views or columns changed (DDL, import, migration): reload metadata.
    DatabaseChanged,
    /// Record writes committed (grid edit, form save, trigger, action query).
    RecordsChanged,
}

#[derive(Default)]
pub struct DocBus;
impl EventEmitter<DocEvent> for DocBus {}

impl DocBus {
    pub fn database_changed(bus: &Entity<Self>, cx: &mut App) {
        bus.update(cx, |_, cx| cx.emit(DocEvent::DatabaseChanged));
    }
    pub fn records_changed(bus: &Entity<Self>, cx: &mut App) {
        bus.update(cx, |_, cx| cx.emit(DocEvent::RecordsChanged));
    }
}
