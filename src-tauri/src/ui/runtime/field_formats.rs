//! The rich-text and attachment inputs of a `FieldView`.
//!
//! Rich text: the toolbar buttons are `<id>:format:<bold|italic|underline|bulleted-list|
//! numbered-list>`, the markup box is `<id>` and the rendered preview `<id>:preview`. Read-only
//! rich text is `<id>`, labelled with its plain text.
//!
//! Attachments: each file is `<id>:file:<slug of its name>` (labelled "name (size)") with
//! `<id>:save:<slug>` and, when editable, `<id>:remove:<slug>`; `<id>:add` picks a file, and
//! `<id>:files-error` shows a failed upload or copy.

use super::FieldView;
use crate::forms::field_values::{AttachmentRef, file_size};
use crate::forms::fields::{attachments_value, parse_attachments, rich_text_plain, value_text};
use crate::forms::richtext::sanitize_rich_text;
use crate::forms::richtext_markup::{toggle_inline, toggle_list};
use crate::record_attachments::{save_record_attachment, upload_record_attachment_file};
use crate::ui::dialogs;
use crate::ui::prelude::*;
use gpui_kit::component::{input::Textarea, text::TextView};

/// Toolbar buttons: key, tooltip, icon, and the inline marker (None for the list buttons).
const FORMATS: [(&str, &str, IconName, Option<&str>); 5] = [
    ("bold", "Bold", IconName::Bold, Some("**")),
    ("italic", "Italic", IconName::Italic, Some("*")),
    ("underline", "Underline", IconName::Underline, Some("++")),
    ("bulleted-list", "Bulleted list", IconName::List, None),
    ("numbered-list", "Numbered list", IconName::ListOrdered, None),
];

/// Distinct id slugs for file names (a repeated name gets its position appended).
fn file_slugs(files: &[AttachmentRef]) -> Vec<String> {
    let mut seen: Vec<String> = vec![];
    for (index, file) in files.iter().enumerate() {
        let mut slug = widgets::slug(&file.name);
        if slug.is_empty() || seen.contains(&slug) {
            slug = format!("{slug}{index}");
        }
        seen.push(slug);
    }
    seen
}

impl FieldView {
    fn apply_format(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(area) = self.area.clone() else { return };
        let (text, selection) = {
            let state = area.read(cx);
            (state.value().to_string(), state.selected_range())
        };
        let marker = FORMATS.iter().find(|f| f.0 == key).and_then(|f| f.3);
        let edit = match (key, marker) {
            (_, Some(marker)) => toggle_inline(&text, selection, marker),
            ("numbered-list", None) => toggle_list(&text, selection, true),
            _ => toggle_list(&text, selection, false),
        };
        area.update(cx, |s, cx| {
            s.replace_all(edit.text, window, cx);
            s.set_selected_range(edit.selection, cx);
            s.focus(window, cx);
        });
    }

    pub(super) fn render_rich_text(&mut self, id: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let theme = cx.theme().clone();
        let html = if props.value.is_null() { String::new() } else { sanitize_rich_text(&value_text(&props.value)) };
        if props.read_only {
            return div()
                .id(id.clone())
                .aria_label(rich_text_plain(&props.value))
                .w_full()
                .text_sm()
                .child(TextView::html(SharedString::from(format!("{id}:view")), html))
                .test_support()
                .into_any_element();
        }
        let Some(area) = self.area.clone() else { return div().into_any_element() };
        let toolbar = h_flex()
            .id(SharedString::from(format!("{id}:toolbar")))
            .aria_label(format!("{} formatting", props.label))
            .gap_0p5()
            .children(FORMATS.iter().map(|(key, name, icon, _)| {
                let key = *key;
                Button::new(SharedString::from(format!("{id}:format:{key}")))
                    .ghost()
                    .xsmall()
                    .icon(icon.clone())
                    .tooltip(name.to_string())
                    .on_click(cx.listener(move |this, _, window, cx| this.apply_format(key, window, cx)))
            }));
        v_flex()
            .gap_1()
            .w_full()
            .child(toolbar)
            .child(
                div()
                    .id(id.clone())
                    .aria_label(props.label.clone())
                    .h(px(84.))
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(Textarea::new(&area).h(relative(1.)))
                    .test_support(),
            )
            .when(!html.is_empty(), |el| {
                el.child(
                    div()
                        .id(SharedString::from(format!("{id}:preview")))
                        .aria_label(rich_text_plain(&props.value))
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme.muted)
                        .text_sm()
                        .child(TextView::html(SharedString::from(format!("{id}:preview-text")), html))
                        .test_support(),
                )
            })
            .into_any_element()
    }

    /// Where the control stores its files: the binding's table (older forms) or the form's.
    fn attachment_target(&self) -> Option<(String, String)> {
        let props = self.props();
        let binding = props.control.binding.as_ref()?;
        let table = binding.table.clone().filter(|t| !t.is_empty()).or_else(|| props.table.clone())?;
        Some((table, binding.column.clone()))
    }

    fn set_files(&mut self, files: &[AttachmentRef], cx: &mut Context<Self>) {
        let on_change = self.props().on_change.clone();
        on_change(attachments_value(files), cx);
        // Leaving the field validates it.
        let on_blur = self.props().on_blur.clone();
        on_blur(cx);
        cx.notify();
    }

    fn add_file(&mut self, cx: &mut Context<Self>) {
        let Some((table, column)) = self.attachment_target() else { return };
        let label = self.props().window_label.to_string();
        self.files_error = None;
        let pick = dialogs::pick_file(cx, "Attach a file", &[]);
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else { return };
            this.uploading = true;
            cx.notify();
            task::spawn_call(
                cx,
                move || upload_record_attachment_file(label, table, column, path),
                |this, result, cx| {
                    this.uploading = false;
                    match result {
                        Ok(file) => {
                            // Read the value now: it may have changed while the file uploaded.
                            let mut files = parse_attachments(&this.props().value);
                            files.push(AttachmentRef {
                                id: file.id,
                                name: file.name,
                                mime: file.mime,
                                size: file.size,
                                sha256: file.sha256,
                            });
                            this.set_files(&files, cx);
                        }
                        Err(e) => {
                            this.files_error = Some(e.message);
                            cx.notify();
                        }
                    }
                },
            );
        });
    }

    fn remove_file(&mut self, file_id: &str, cx: &mut Context<Self>) {
        let mut files = parse_attachments(&self.props().value);
        files.retain(|f| f.id != file_id);
        self.files_error = None;
        self.set_files(&files, cx);
    }

    fn save_file(&mut self, file: AttachmentRef, cx: &mut Context<Self>) {
        let label = self.props().window_label.to_string();
        self.files_error = None;
        let pick = dialogs::save_file(cx, "Save attachment", &file.name, &[]);
        task::after(cx, pick, move |_, path, cx| {
            let Some(path) = path else { return };
            task::spawn_call(
                cx,
                move || save_record_attachment(label, file.id, path),
                |this, result, cx| {
                    if let Err(e) = result {
                        this.files_error = Some(e.message);
                        cx.notify();
                    }
                },
            );
        });
    }

    pub(super) fn render_attachments(&mut self, id: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let files = parse_attachments(&props.value);
        let slugs = file_slugs(&files);
        let theme = cx.theme().clone();
        let editable = !props.read_only && self.attachment_target().is_some();
        let rows = files.into_iter().zip(slugs).map(|(file, slug)| {
            let (saved, removed) = (file.clone(), file.id.clone());
            h_flex()
                .gap_1()
                .child(Icon::new(IconName::Paperclip).xsmall().text_color(theme.muted_foreground))
                .child(widgets::styled_text(
                    SharedString::from(format!("{id}:file:{slug}")),
                    format!("{} ({})", file.name, file_size(file.size)),
                    |d| d.text_sm(),
                ))
                .child(
                    Button::new(SharedString::from(format!("{id}:save:{slug}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Download)
                        .tooltip(format!("Save {}", file.name))
                        .on_click(cx.listener(move |this, _, _, cx| this.save_file(saved.clone(), cx))),
                )
                .when(!props.read_only, |el| {
                    el.child(
                        Button::new(SharedString::from(format!("{id}:remove:{slug}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove {}", file.name))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_file(&removed, cx))),
                    )
                })
        });
        let empty = parse_attachments(&props.value).is_empty();
        v_flex()
            .id(id.clone())
            .aria_label(format!("{} files", props.label))
            .gap_0p5()
            .children(rows)
            .when(empty, |el| el.child(div().text_sm().text_color(theme.muted_foreground).child("No files")))
            .when(editable, |el| {
                el.child(
                    h_flex().child(
                        Button::new(SharedString::from(format!("{id}:add")))
                            .outline()
                            .xsmall()
                            .icon(IconName::Paperclip)
                            .label(if self.uploading { "Uploading…" } else { "Add file" })
                            .disabled(self.uploading)
                            .on_click(cx.listener(|this, _, _, cx| this.add_file(cx))),
                    ),
                )
            })
            .when_some(self.files_error.clone(), |el, error| {
                el.child(widgets::styled_text(
                    SharedString::from(format!("{id}:files-error")),
                    error,
                    |d| d.text_sm().text_color(theme.danger),
                ))
            })
            .test_support()
    }
}
