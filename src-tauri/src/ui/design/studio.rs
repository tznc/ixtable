//! `DesignStudio`: the form designer (port of `DesignStudio.tsx`). It owns what is selected
//! (form, control, active tab pages, view) and renders the three panes: the form list and
//! component palette, the canvas (or navigation editor, or preview) and the properties panel.
//! Every definition edit goes through `ConfigStore::update`, so undo and redo cover it.
//!
//! The pieces live in sibling files as `impl DesignStudio` blocks: `form_list.rs`,
//! `canvas.rs`, `control_props.rs`, `kind_props.rs`, `form_props.rs`, `layout_settings.rs`,
//! `navigation.rs`.
//!
//! Test ids: `design-studio` (the studio, labelled "Form builder"), `properties` (labelled
//! "Properties"), `properties-title` ("Control" or "Form"), `back-to-form`, the palette
//! buttons `add-<kind>` (`add-text`, `add-related-list`, `add-boolean`, ...), the view tabs
//! `tab-form`, `tab-navigation`, `tab-preview`, `preview-mode`, `form-hint`, `no-form`
//! and `no-controls`.

use super::{
    form_list::ListState,
    inputs::{Drafts, Fields},
    preview::DesignPreview,
};
use crate::archive::{Attachment, DocumentConfig};
use crate::design::{Control, ControlKind, DesignSchema, Form, FormMode, SourceKind};
use crate::forms::{
    legacy_ids::LEGACY_ID,
    operations::{add_control, target_container},
    schema::{CONTROL_KINDS, FORM_MODES, control_kind_label, form_mode_key, form_mode_label, form_table},
};
use crate::ui::{
    events::DocEvent,
    prelude::*,
    reports::fields::{choice, pairs},
    shell::{ModeId, RevealTarget, Shell},
    store::StoreEvent,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Form,
    Navigation,
    Preview,
}

const VIEWS: [(&str, &str); 3] = [("form", "Layout"), ("navigation", "Navigation"), ("preview", "Preview")];

/// `relatedList` as `related-list`: the element id suffix of a control kind.
pub fn kind_id(kind: ControlKind) -> String {
    let key = serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    let mut out = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

pub struct DesignStudio {
    pub(super) shell: Entity<Shell>,
    pub form_id: Option<String>,
    pub selected: Option<String>,
    pub active_tabs: HashMap<String, String>,
    pub view: View,
    pub preview_mode: FormMode,
    pub(super) fields: Fields<DesignStudio>,
    pub(super) drafts: Drafts<DesignStudio>,
    columns: HashMap<String, Vec<String>>,
    loading: HashSet<String>,
    pub(super) list: ListState,
    preview: Option<Entity<DesignPreview>>,
    pub(super) assets: Option<Vec<Attachment>>,
    assets_loading: bool,
    pub(super) previews: HashMap<String, super::asset_picker::Preview>,
    wired: bool,
    _subscriptions: Vec<Subscription>,
}

fn has_legacy_ids(design: &DesignSchema) -> bool {
    fn any(items: &[crate::design::NavigationItem]) -> bool {
        items.iter().any(|i| i.id == LEGACY_ID || any(&i.children))
    }
    design.forms.iter().any(|f| f.id == LEGACY_ID) || any(&design.navigation)
}

impl DesignStudio {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        // The shell is being updated while it builds this view, so it is read from the first
        // render on (`wire`).
        let subscriptions = vec![cx.observe(&shell, |_, _, cx| cx.notify())];
        Self {
            shell,
            form_id: None,
            selected: None,
            active_tabs: HashMap::new(),
            view: View::Form,
            preview_mode: FormMode::List,
            fields: Fields::default(),
            drafts: Drafts::default(),
            columns: HashMap::new(),
            loading: HashSet::new(),
            list: ListState::default(),
            preview: None,
            assets: None,
            assets_loading: false,
            previews: HashMap::new(),
            wired: false,
            _subscriptions: subscriptions,
        }
    }

    /// Subscribes to the document's store and bus (once).
    fn wire(&mut self, cx: &mut Context<Self>) {
        if self.wired {
            return;
        }
        self.wired = true;
        let (store, bus) = {
            let s = self.shell.read(cx);
            (s.store.clone(), s.bus.clone())
        };
        self._subscriptions.push(cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
            if matches!(e, StoreEvent::Changed) {
                this.upgrade_legacy_ids(cx);
            }
        }));
        self._subscriptions.push(cx.subscribe(&bus, |this, _, e: &DocEvent, cx| {
            if matches!(e, DocEvent::DatabaseChanged) {
                this.columns.clear();
                this.assets = None;
                cx.notify();
            }
        }));
        self.upgrade_legacy_ids(cx);
    }

    /// A legacy `main` form or navigation id (Rust rewrites it on open; this covers YAML
    /// edits) is replaced without an undo step.
    fn upgrade_legacy_ids(&mut self, cx: &mut Context<Self>) {
        if !has_legacy_ids(&self.shell.read(cx).config(cx).design) {
            return;
        }
        let store = self.shell.read(cx).store.clone();
        let _ = store.update(cx, |s, cx| {
            s.update_with(
                "Upgrade form ids",
                false,
                |config| {
                    let _ = crate::design::upgrade::rekey_legacy_ids(config);
                },
                cx,
            )
        });
    }

    // ---- Model ------------------------------------------------------------------------

    pub fn config<'a>(&self, cx: &'a App) -> &'a DocumentConfig {
        self.shell.read(cx).config(cx)
    }

    pub fn design<'a>(&self, cx: &'a App) -> &'a DesignSchema {
        &self.config(cx).design
    }

    /// The form shown: the chosen one, else the first.
    pub fn form(&self, cx: &App) -> Option<Form> {
        let forms = &self.design(cx).forms;
        forms.iter().find(|f| Some(&f.id) == self.form_id.as_ref()).or(forms.first()).cloned()
    }

    pub fn selected_control(&self, cx: &App) -> Option<Control> {
        let selected = self.selected.as_ref()?;
        self.form(cx)?.controls.into_iter().find(|c| &c.id == selected)
    }

    pub fn tables(&self, cx: &App) -> Vec<String> {
        self.shell.read(cx).objects.iter().filter(|o| o.object_type == "table").map(|o| o.name.clone()).collect()
    }

    /// Applies `mutate` to the stored config as one undo step named `label`.
    pub(super) fn edit(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut DocumentConfig) + 'static,
        cx: &mut Context<Self>,
    ) {
        let store = self.shell.read(cx).store.clone();
        if let Err(e) = store.update(cx, |s, cx| s.update(label, mutate, cx)) {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
    }

    pub(super) fn edit_design(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut DesignSchema) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.edit(label, move |config| mutate(&mut config.design), cx);
    }

    /// Applies `mutate` to the shown form.
    pub(super) fn edit_form(&mut self, label: &str, mutate: impl FnOnce(&mut Form) + 'static, cx: &mut Context<Self>) {
        let Some(id) = self.form(cx).map(|f| f.id) else {
            return;
        };
        self.edit_design(
            label,
            move |design| {
                if let Some(form) = design.forms.iter_mut().find(|f| f.id == id) {
                    mutate(form);
                }
            },
            cx,
        );
    }

    /// Applies `mutate` to a control of the shown form.
    pub(super) fn edit_control(
        &mut self,
        control_id: &str,
        label: &str,
        mutate: impl FnOnce(&mut Control) + 'static,
        cx: &mut Context<Self>,
    ) {
        let id = control_id.to_string();
        self.edit_form(
            label,
            move |form| {
                if let Some(control) = form.controls.iter_mut().find(|c| c.id == id) {
                    mutate(control);
                }
            },
            cx,
        );
    }

    /// The text shown in the input `id` (a field or a draft).
    #[cfg(test)]
    pub fn input_text(&self, id: &str, cx: &App) -> Option<String> {
        self.fields.value(id, cx).or_else(|| self.drafts.value(id, cx))
    }

    /// Whether the columns of `table` have been read (tests wait on this).
    #[cfg(test)]
    pub fn has_columns(&self, table: &str) -> bool {
        self.columns.contains_key(table)
    }

    /// Column names of `table` (empty while loading or unknown).
    pub(super) fn columns_of(&mut self, table: Option<&str>, cx: &mut Context<Self>) -> Vec<String> {
        let Some(table) = table.filter(|t| !t.is_empty()) else {
            return vec![];
        };
        if let Some(columns) = self.columns.get(table) {
            return columns.clone();
        }
        if self.loading.insert(table.to_string()) {
            let label = self.shell.read(cx).label().to_string();
            let name = table.to_string();
            task::spawn_call(
                cx,
                move || {
                    crate::inspect_table(label, name.clone(), None)
                        .map(|schema| schema.columns.into_iter().map(|c| c.name).collect::<Vec<_>>())
                        .unwrap_or_default()
                },
                {
                    let table = table.to_string();
                    move |this, columns, cx| {
                        this.loading.remove(&table);
                        this.columns.insert(table, columns);
                        cx.notify();
                    }
                },
            );
        }
        vec![]
    }

    /// Image assets of the application, loaded once.
    pub(super) fn image_assets(&mut self, cx: &mut Context<Self>) -> Option<Vec<Attachment>> {
        if self.assets.is_none() && !self.assets_loading {
            self.assets_loading = true;
            let label = self.shell.read(cx).label().to_string();
            task::spawn_call(
                cx,
                move || crate::list_attachments(label).unwrap_or_default(),
                |this, assets, cx| {
                    this.assets = Some(assets);
                    this.assets_loading = false;
                    cx.notify();
                },
            );
        }
        self.assets.clone()
    }

    // ---- Selection --------------------------------------------------------------------

    pub fn select_form(&mut self, id: &str, cx: &mut Context<Self>) {
        self.form_id = Some(id.to_string());
        self.selected = None;
        cx.notify();
    }

    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        cx.notify();
    }

    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.view = view;
        cx.notify();
    }

    /// Opens the form and control another mode asked to reveal.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(ModeId::Design));
        let Some(RevealTarget::Design { object_id, element_id }) = request else {
            return;
        };
        let parent = self
            .design(cx)
            .forms
            .iter()
            .find(|f| f.id == object_id)
            .and_then(|f| f.controls.iter().find(|c| Some(&c.id) == element_id.as_ref()))
            .and_then(|c| c.parent.clone());
        self.form_id = Some(object_id);
        self.selected = element_id;
        self.view = View::Form;
        if let Some(parent) = parent {
            if let Some(tab) = parent.tab {
                self.active_tabs.insert(parent.id, tab);
            }
        }
        cx.notify();
    }

    /// Adds a control of `kind` to the selected container (or the form grid).
    pub fn add(&mut self, kind: ControlKind, cx: &mut Context<Self>) {
        let Some(form) = self.form(cx) else {
            return;
        };
        let parent = target_container(&form, self.selected.as_deref(), &self.active_tabs);
        let (_, created) = add_control(&form, kind, parent.as_ref());
        self.selected = Some(created.id.clone());
        self.edit_form("Add control", move |f| f.controls.push(created), cx);
    }

    /// Sets a control's placement (resize or move on the canvas).
    pub fn place(&mut self, id: &str, placement: crate::design::Placement, cx: &mut Context<Self>) {
        self.edit_control(id, "Resize control", move |c| c.placement = placement, cx);
    }

    // ---- Rendering ----------------------------------------------------------------------

    fn render_palette(&self, has_form: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .id("palette")
            .aria_label("Components")
            .gap_1()
            .child(div().text_xs().text_color(muted).child("COMPONENTS"))
            .children(CONTROL_KINDS.into_iter().map(|kind| {
                Button::new(SharedString::from(format!("add-{}", kind_id(kind))))
                    .outline()
                    .small()
                    .justify_start()
                    .label(control_kind_label(kind))
                    .tooltip(format!("Add {}", control_kind_label(kind)))
                    .disabled(!has_form)
                    .on_click(cx.listener(move |this, _, _, cx| this.add(kind, cx)))
            }))
    }

    fn render_toolbar(&mut self, form: Option<&Form>, cx: &mut Context<Self>) -> impl IntoElement {
        let current = match self.view {
            View::Form => "form",
            View::Navigation => "navigation",
            View::Preview => "preview",
        };
        let entity = cx.entity();
        let modes: Vec<FormMode> = match form {
            Some(f) if !f.modes.is_empty() => f.modes.clone(),
            _ => vec![FormMode::List],
        };
        let mode = if modes.contains(&self.preview_mode) { self.preview_mode } else { modes[0] };
        let options: Vec<(String, String)> = FORM_MODES
            .into_iter()
            .filter(|m| modes.contains(m))
            .map(|m| (form_mode_key(m).to_string(), form_mode_label(m).to_string()))
            .collect();
        let entity_for_mode = cx.entity();
        h_flex()
            .id("design-toolbar")
            .aria_label("Designer view")
            .items_center()
            .child(widgets::tab_strip(
                "design-views",
                &VIEWS,
                current,
                move |key, _, cx| {
                    let view = match key {
                        "navigation" => View::Navigation,
                        "preview" => View::Preview,
                        _ => View::Form,
                    };
                    entity.update(cx, |this, cx| this.set_view(view, cx));
                },
                cx,
            ))
            .when(self.view == View::Preview, |el| {
                el.child(widgets::field(
                    "Preview mode",
                    choice("preview-mode", form_mode_key(mode), options, "", move |value, _, cx| {
                        let mode = crate::forms::schema::form_mode_from_key(value);
                        entity_for_mode.update(cx, |this, cx| {
                            if let Some(mode) = mode {
                                this.preview_mode = mode;
                                cx.notify();
                            }
                        });
                    }),
                    cx,
                ))
            })
    }

    /// The preview entity for the shown form and mode, rebuilt when either changes.
    fn preview_for(&mut self, form: &Form, mode: FormMode, window: &mut Window, cx: &mut Context<Self>) -> Entity<DesignPreview> {
        if let Some(preview) = &self.preview {
            let same = cx.read_entity(preview, |p, _| p.form_id() == form.id && p.mode() == mode);
            if same {
                return preview.clone();
            }
        }
        let (shell, id) = (self.shell.clone(), form.id.clone());
        let preview = cx.new(|cx| DesignPreview::new(shell, &id, mode, window, cx));
        self.preview = Some(preview.clone());
        preview
    }

    /// The preview entity, if the preview is showing.
    pub fn preview(&self) -> Option<Entity<DesignPreview>> {
        self.preview.clone()
    }

    fn render_center(&mut self, form: Option<Form>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match (self.view, form) {
            (View::Navigation, _) => self.render_navigation(window, cx).into_any_element(),
            (View::Preview, Some(form)) => {
                let modes = if form.modes.is_empty() { vec![FormMode::List] } else { form.modes.clone() };
                let mode = if modes.contains(&self.preview_mode) { self.preview_mode } else { modes[0] };
                self.preview_for(&form, mode, window, cx).into_any_element()
            }
            (View::Form, Some(form)) => {
                let theme = cx.theme();
                let muted = theme.muted_foreground;
                let kicker = match form.source.as_ref().map(|s| s.kind) {
                    Some(SourceKind::Query) => "READ-ONLY FORM".to_string(),
                    _ => form_table(&form).unwrap_or("UNBOUND").to_uppercase(),
                };
                let empty = form.controls.is_empty();
                v_flex()
                    .id("form-card")
                    .gap_2()
                    .p_3()
                    .child(div().text_xs().text_color(muted).child(kicker))
                    .child(widgets::styled_text("form-title", form.name.clone(), |d| {
                        d.text_xl().font_weight(FontWeight::SEMIBOLD)
                    }))
                    .child(widgets::styled_text(
                        "form-hint",
                        "Enter selects a control, Alt+Arrow resizes, Alt+Shift+Arrow moves.",
                        move |d| d.text_sm().text_color(muted),
                    ))
                    .child(self.render_canvas(&form, None, None, window, cx))
                    .when(empty, |el| {
                        el.child(widgets::text("no-controls", "Add a component to start building this form."))
                    })
                    .into_any_element()
            }
            (_, None) => widgets::text("no-form", "Create a form or generate one from a table.").into_any_element(),
        }
    }

    fn render_properties_panel(&mut self, form: Option<Form>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut panel = v_flex()
            .id("properties")
            .aria_label("Properties")
            .w(px(340.))
            .h_full()
            .flex_none()
            .gap_2()
            .p_3()
            .border_l_1()
            .border_color(cx.theme().border)
            .overflow_y_scroll()
            .child(div().text_xs().text_color(muted).child("PROPERTIES"));
        if let Some(form) = form {
            let control = self.selected_control(cx);
            panel = panel
                .child(widgets::styled_text(
                    "properties-title",
                    if control.is_some() { "Control" } else { "Form" },
                    |d| d.font_weight(FontWeight::BOLD),
                ))
                .when(control.is_some(), |el| {
                    el.child(
                        Button::new("back-to-form")
                            .link()
                            .small()
                            .label("Back to form properties")
                            .on_click(cx.listener(|this, _, _, cx| this.select(None, cx))),
                    )
                });
            panel = match control {
                Some(control) => panel
                    .child(self.render_control_properties(&form, &control, window, cx))
                    .children(self.render_container_grid(&form, &control, window, cx)),
                None => panel.child(self.render_form_properties(&form, window, cx)),
            };
        }
        panel.test_support().into_any_element()
    }
}

impl Render for DesignStudio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The shell is being updated while it builds this view, so reveals are taken here.
        self.wire(cx);
        self.take_reveal(cx);
        let form = self.form(cx);
        let has_form = form.is_some();
        let border = cx.theme().border;
        let left = v_flex()
            .id("design-left")
            .w(px(250.))
            .h_full()
            .flex_none()
            .gap_3()
            .p_3()
            .border_r_1()
            .border_color(border)
            .overflow_y_scroll()
            .child(self.render_form_list(form.as_ref().map(|f| f.id.as_str()), window, cx))
            .child(self.render_palette(has_form, cx));
        let toolbar = self.render_toolbar(form.as_ref(), cx);
        let center = div()
            .id("design-center")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(toolbar)
            .child(self.render_center(form.clone(), window, cx));
        let properties = self.render_properties_panel(form, window, cx);
        v_flex()
            .size_full()
            .child(widgets::titlebar("Project / Design", "Form builder", cx))
            .child(
                h_flex()
                    .id("design-studio")
                    .aria_label("Form builder")
                    .flex_1()
                    .min_h_0()
                    .child(left)
                    .child(center)
                    .child(properties)
                    .test_support(),
            )
    }
}

/// Plain choice options from names.
pub(super) fn same(values: &[String]) -> Vec<(String, String)> {
    values.iter().map(|v| (v.clone(), v.clone())).collect()
}

/// Shorthand for fixed (value, label) pairs.
pub(super) fn options(values: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs(values)
}
