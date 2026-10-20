//! The rich-text input of a `FieldView`: a Markdown box with a toolbar and a formatted preview.
//!
//! The toolbar buttons are `<id>:format:<key>` (keys in `FORMATS`), the Markdown box is `<id>`
//! and the rendered preview `<id>:preview`. Read-only rich text is `<id>`, labelled with its
//! plain text. `<id>:image-error` shows a failed image upload.
//!
//! Images (`docs/decisions/field-formats.md`): pasting an image, or the Image button, uploads it
//! to the form's attachment field and inserts `![…](attachment:<name>)`. The preview shows
//! attachment images from the record's files and downloads web images; until an image has
//! loaded it shows its description.

use super::FieldView;
use crate::forms::field_values::AttachmentRef;
use crate::forms::fields::{attachments_value, parse_attachments, rich_text_plain, value_text};
use crate::forms::richtext::{ATTACHMENT_SCHEME, rich_text_display};
use crate::forms::richtext_edit::{LineKind, code_block, divider, image, link, table, toggle_inline, toggle_lines};
use crate::forms::richtext_images::{
    attachment_for, attachment_src, fetch_web_image, image_extension, image_mime, image_sources, pasted_image_name,
};
use crate::record_attachments::{read_record_attachment, upload_record_attachment};
use crate::ui::dialogs::{self, FileFilter};
use crate::ui::prelude::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use gpui_kit::component::{input::Textarea, text::TextView};

const IMAGES: &[FileFilter] =
    &[FileFilter { name: "Images", extensions: &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"] }];

#[derive(Clone, Copy)]
enum Format {
    Inline(&'static str, &'static str),
    Lines(LineKind),
    Link,
    Image,
    Table,
    CodeBlock,
    Divider,
}

/// Toolbar buttons: key, tooltip, icon and what the button does.
const FORMATS: [(&str, &str, IconName, Format); 15] = [
    ("bold", "Bold", IconName::Bold, Format::Inline("**", "**")),
    ("italic", "Italic", IconName::Italic, Format::Inline("*", "*")),
    ("underline", "Underline", IconName::Underline, Format::Inline("<u>", "</u>")),
    ("strikethrough", "Strikethrough", IconName::Strikethrough, Format::Inline("~~", "~~")),
    ("code", "Code", IconName::Code, Format::Inline("`", "`")),
    ("heading", "Heading", IconName::Heading, Format::Lines(LineKind::Heading(2))),
    ("quote", "Quote", IconName::TextQuote, Format::Lines(LineKind::Quote)),
    ("bulleted-list", "Bulleted list", IconName::List, Format::Lines(LineKind::Bullet)),
    ("numbered-list", "Numbered list", IconName::ListOrdered, Format::Lines(LineKind::Numbered)),
    ("task-list", "Task list", IconName::ListTodo, Format::Lines(LineKind::Task)),
    ("link", "Link", IconName::Link, Format::Link),
    ("image", "Image", IconName::Image, Format::Image),
    ("table", "Table", IconName::Table, Format::Table),
    ("code-block", "Code block", IconName::SquareCode, Format::CodeBlock),
    ("divider", "Divider", IconName::Minus, Format::Divider),
];

/// An image the preview shows: a data URL once loaded.
#[derive(Clone)]
pub(super) enum RichImage {
    Loading,
    Ready(String),
    Failed,
}

fn data_url(bytes: &[u8], mime: &str) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

impl FieldView {
    /// Every file in the record's attachment fields.
    fn record_files(&self) -> Vec<AttachmentRef> {
        let props = self.props();
        props.key_values.values().chain([&props.value]).flat_map(parse_attachments).collect()
    }

    /// Starts loading the images `html` shows that are not loaded or loading yet.
    fn load_images(&mut self, html: &str, cx: &mut Context<Self>) {
        let sources: Vec<String> = image_sources(html).into_iter().filter(|s| !self.images.contains_key(s)).collect();
        if sources.is_empty() {
            return;
        }
        let files = self.record_files();
        let label = self.props().window_label.to_string();
        for src in sources {
            let job: Box<dyn FnOnce() -> Option<String> + Send> = if src.starts_with(ATTACHMENT_SCHEME) {
                // A file the record does not hold yet (still uploading) is tried again later.
                let Some(file) = attachment_for(&src, &files).cloned() else { continue };
                let label = label.clone();
                Box::new(move || {
                    let content = read_record_attachment(label, file.id).ok()?;
                    let bytes = STANDARD.decode(content.content_base64.as_bytes()).ok()?;
                    let mime = image_mime(&bytes)?;
                    Some(data_url(&bytes, mime))
                })
            } else {
                let url = src.clone();
                Box::new(move || {
                    let bytes = fetch_web_image(&url).ok()?;
                    Some(data_url(&bytes, image_mime(&bytes)?))
                })
            };
            self.images.insert(src.clone(), RichImage::Loading);
            task::spawn_call(cx, job, move |this, loaded, cx| {
                this.images.insert(src, loaded.map_or(RichImage::Failed, RichImage::Ready));
                cx.notify();
            });
        }
    }

    /// The HTML the text view shows for `html`, with loaded images inlined.
    fn display_html(&mut self, html: &str, cx: &mut Context<Self>) -> String {
        self.load_images(html, cx);
        let images = &self.images;
        rich_text_display(html, &|src| match images.get(src) {
            Some(RichImage::Ready(data)) => Some(data.clone()),
            _ => None,
        })
    }

    fn apply_format(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(area) = self.area.clone() else { return };
        let Some(format) = FORMATS.iter().find(|f| f.0 == key).map(|f| f.3) else { return };
        if let Format::Image = format {
            self.pick_image(window, cx);
            return;
        }
        let (text, selection) = {
            let state = area.read(cx);
            (state.value().to_string(), state.selected_range())
        };
        let edit = match format {
            Format::Inline(open, close) => toggle_inline(&text, selection, open, close),
            Format::Lines(kind) => toggle_lines(&text, selection, kind),
            Format::Link => link(&text, selection),
            Format::Table => table(&text, selection),
            Format::CodeBlock => code_block(&text, selection),
            Format::Divider | Format::Image => divider(&text, selection),
        };
        area.update(cx, |s, cx| {
            s.replace_all(edit.text, window, cx);
            s.set_selected_range(edit.selection, cx);
            s.focus(window, cx);
        });
    }

    /// The table and attachment column that store inserted images.
    fn image_target(&self) -> Option<(String, String)> {
        let props = self.props();
        Some((props.table.clone()?, props.attachment_column.clone()?))
    }

    fn pick_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.image_error = None;
        let pick = dialogs::pick_file(cx, "Insert an image", IMAGES);
        task::after_in(window, cx, pick, move |this, path, window, cx| {
            let Some(path) = path else { return };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            match std::fs::read(&path) {
                Ok(bytes) => this.upload_image(bytes, name, window, cx),
                Err(e) => {
                    this.image_error = Some(e.to_string());
                    cx.notify();
                }
            }
        });
    }

    /// Takes a paste that holds an image (or a copied image file); other pastes insert text.
    pub(super) fn paste_image(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(picture) => {
                    self.upload_image(picture.bytes().to_vec(), None, window, cx);
                    return true;
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    let Some(path) = paths.paths().iter().find(|p| {
                        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                        IMAGES[0].extensions.contains(&ext.as_str())
                    }) else {
                        continue;
                    };
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    match std::fs::read(path) {
                        Ok(bytes) => self.upload_image(bytes, name, window, cx),
                        Err(e) => {
                            self.image_error = Some(e.to_string());
                            cx.notify();
                        }
                    }
                    return true;
                }
                ClipboardEntry::String(_) => {}
            }
        }
        false
    }

    /// Stores `bytes` in the form's attachment field and inserts the image at the caret.
    fn upload_image(&mut self, bytes: Vec<u8>, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.image_error = None;
        let Some((table, column)) = self.image_target() else {
            self.image_error = Some("Add an attachment field to this form to insert images".into());
            cx.notify();
            return;
        };
        let Some(mime) = image_mime(&bytes) else {
            self.image_error = Some("The file is not an image".into());
            cx.notify();
            return;
        };
        let current = self.props().key_values.get(&column).map(parse_attachments).unwrap_or_default();
        let name = name.unwrap_or_else(|| {
            pasted_image_name(&self.record_files(), image_extension(mime), chrono::Local::now().naive_local())
        });
        let shown = data_url(&bytes, mime);
        let label = self.props().window_label.to_string();
        let encoded = STANDARD.encode(&bytes);
        self.uploading = true;
        cx.notify();
        task::spawn_call_in(
            window,
            cx,
            move || upload_record_attachment(label, table, column.clone(), name, Some(mime.to_string()), encoded).map(|f| (f, column)),
            move |this, result, window, cx| {
                this.uploading = false;
                let (file, column) = match result {
                    Ok(done) => done,
                    Err(e) => {
                        this.image_error = Some(e.message);
                        cx.notify();
                        return;
                    }
                };
                let mut files = current;
                let src = attachment_src(&file.name);
                files.push(AttachmentRef { id: file.id, name: file.name.clone(), mime: file.mime, size: file.size, sha256: file.sha256 });
                let on_keys = this.props().on_keys.clone();
                on_keys(vec![(column, attachments_value(&files))], cx);
                this.images.insert(src.clone(), RichImage::Ready(shown));
                let Some(area) = this.area.clone() else { return };
                let (text, selection) = {
                    let state = area.read(cx);
                    (state.value().to_string(), state.selected_range())
                };
                let alt = file.name.rsplit_once('.').map_or(file.name.as_str(), |(stem, _)| stem).to_string();
                let edit = image(&text, selection, &alt, &src);
                area.update(cx, |s, cx| {
                    s.replace_all(edit.text, window, cx);
                    s.set_selected_range(edit.selection, cx);
                    s.focus(window, cx);
                });
            },
        );
    }

    pub(super) fn render_rich_text(&mut self, id: &SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        let props = self.props().clone();
        let theme = cx.theme().clone();
        let html = if props.value.is_null() { String::new() } else { self.display_html(&value_text(&props.value), cx) };
        if props.read_only {
            return div()
                .id(id.clone())
                .aria_label(rich_text_plain(&props.value))
                .w_full()
                .ty(Type::Body)
                .child(TextView::html(SharedString::from(format!("{id}:view")), html))
                .test_support()
                .into_any_element();
        }
        let Some(area) = self.area.clone() else { return div().into_any_element() };
        let can_insert_image = self.image_target().is_some();
        let toolbar = h_flex()
            .id(SharedString::from(format!("{id}:toolbar")))
            .aria_label(format!("{} formatting", props.label))
            .flex_wrap()
            .gap_0p5()
            .children(FORMATS.iter().filter(|f| can_insert_image || !matches!(f.3, Format::Image)).map(
                |(key, name, icon, _)| {
                    let key = *key;
                    Button::new(SharedString::from(format!("{id}:format:{key}")))
                        .ghost()
                        .xsmall()
                        .icon(*icon)
                        .tooltip(name.to_string())
                        .disabled(key == "image" && self.uploading)
                        .on_click(cx.listener(move |this, _, window, cx| this.apply_format(key, window, cx)))
                },
            ));
        let field = cx.entity().downgrade();
        v_flex()
            .gap_1()
            .w_full()
            .child(toolbar)
            .child(
                div()
                    .id(id.clone())
                    .aria_label(props.label.clone())
                    .h(px(140.))
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(Textarea::new(&area).h(relative(1.)).on_paste(move |item, window, cx| {
                        field.update(cx, |this, cx| this.paste_image(item, window, cx)).unwrap_or(false)
                    }))
                    .test_support(),
            )
            .when_some(self.image_error.clone(), |el, error| {
                el.child(widgets::styled_text(
                    SharedString::from(format!("{id}:image-error")),
                    error,
                    |d| d.ty(Type::Body).text_color(theme.danger),
                ))
            })
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
                        .ty(Type::Body)
                        .child(TextView::html(SharedString::from(format!("{id}:preview-text")), html))
                        .test_support(),
                )
            })
            .into_any_element()
    }
}
