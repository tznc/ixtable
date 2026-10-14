//! "Generate app from tables" and "Create forms for <table>" (port of `useGenerateApp` in
//! `src/design/generateApp.ts` and `GenerateAppButton.tsx`). One undoable config edit adds the
//! forms and navigation every table is still missing.
//!
//! Test ids: the button is found by the id it is built with; its error line is `<id>-error`.

use crate::data::TableSchema;
use crate::forms::generate_app::add_generated_app;
use crate::manager::AppError;
use crate::ui::{prelude::*, shell::Shell, store::ConfigStore};

/// Schemas of every table in the open database (views are not generated).
pub fn table_schemas(label: &str) -> Result<Vec<TableSchema>, AppError> {
    crate::list_database_objects(label.to_string())?
        .into_iter()
        .filter(|object| object.object_type == "table")
        .map(|object| crate::inspect_table(label.to_string(), object.name, None))
        .collect()
}

/// Adds what `only` (every table when None) is missing as one undoable edit. Returns how many
/// forms and navigation pages were added (0 when nothing was missing: no empty undo step).
pub fn apply_generated_app(
    store: &Entity<ConfigStore>,
    schemas: &[TableSchema],
    only: Option<&[String]>,
    cx: &mut App,
) -> Result<usize, AppError> {
    let (design, pages, entities) = {
        let config = store.read(cx).config();
        (config.design.clone(), config.pages.clone(), config.entities.clone())
    };
    let Some(next) = add_generated_app(&design, &pages, schemas, only, &entities) else {
        return Ok(0);
    };
    let added = (next.forms.len() + next.navigation.len()).saturating_sub(design.forms.len() + design.navigation.len());
    let label = match only {
        Some([table]) => format!("Create forms for {table}"),
        _ => "Generate app from tables".to_string(),
    };
    store.update(cx, |s, cx| s.update(&label, move |c| {
        c.design = next;
        crate::pages::upgrade::sync(c);
    }, cx))?;
    Ok(added)
}

/// Reads the schemas off the UI thread, then generates; `done` gets the number added.
pub fn generate_app<V: 'static>(
    store: Entity<ConfigStore>,
    label: String,
    only: Option<Vec<String>>,
    cx: &mut Context<V>,
    done: impl FnOnce(&mut V, Result<usize, AppError>, &mut Context<V>) + 'static,
) {
    task::spawn_call(
        cx,
        move || table_schemas(&label),
        move |this, schemas, cx| {
            let result = schemas.and_then(|schemas| apply_generated_app(&store, &schemas, only.as_deref(), cx));
            done(this, result, cx);
        },
    );
}

pub enum GenerateEvent {
    /// The number of forms and pages added.
    Done(usize),
}

/// A button that generates forms and navigation for every table (or only `tables`) still
/// missing them. Used by Data mode's "create forms" offer.
pub struct GenerateAppButton {
    shell: Entity<Shell>,
    id: SharedString,
    label: String,
    tables: Option<Vec<String>>,
    busy: bool,
    error: Option<String>,
}
impl EventEmitter<GenerateEvent> for GenerateAppButton {}

impl GenerateAppButton {
    pub fn new(
        shell: Entity<Shell>,
        id: impl Into<SharedString>,
        label: impl Into<String>,
        tables: Option<Vec<String>>,
    ) -> Self {
        Self { shell, id: id.into(), label: label.into(), tables, busy: false, error: None }
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        cx.notify();
        let (store, label) = {
            let shell = self.shell.read(cx);
            (shell.store.clone(), shell.label().to_string())
        };
        generate_app(store, label, self.tables.clone(), cx, |this, result, cx| {
            this.busy = false;
            match result {
                Ok(added) => {
                    this.error = None;
                    cx.emit(GenerateEvent::Done(added));
                }
                Err(e) => this.error = Some(e.message),
            }
            cx.notify();
        });
    }
}

impl Render for GenerateAppButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let danger = cx.theme().danger;
        let error_id = SharedString::from(format!("{}-error", self.id));
        v_flex()
            .gap_1()
            .child(
                Button::new(self.id.clone())
                    .outline()
                    .small()
                    .icon(IconName::WandSparkles)
                    .label(self.label.clone())
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.run(cx))),
            )
            .children(self.error.clone().map(|message| {
                widgets::styled_text(error_id, message, move |d| d.text_xs().text_color(danger))
            }))
    }
}
