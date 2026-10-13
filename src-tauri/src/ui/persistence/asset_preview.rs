//! Read-only view of a text asset, one block per titled section (VBA modules).
//! Port of src/persistence/AssetPreview.tsx.
//!
//! Test ids: card `asset-preview` (label "Preview of <name>"), `close-preview`,
//! `preview-error`, `preview-loading`, `preview-nav:<i>`, `preview-section:<i>` (label =
//! the section text), `preview-title:<i>`.

use super::{eid, text_sections::text_sections};
use crate::{
    archive::Attachment,
    ui::{forms, prelude::*},
};

pub enum PreviewEvent {
    Close,
}

pub struct AssetPreview {
    asset: Attachment,
    content: Option<String>,
    error: String,
}
impl EventEmitter<PreviewEvent> for AssetPreview {}

impl AssetPreview {
    pub fn new(label: String, asset: Attachment, cx: &mut Context<Self>) -> Self {
        let id = asset.id.clone();
        task::spawn_call(
            cx,
            move || crate::asset_data::read_asset_text(label, id),
            |this, result, cx| {
                match result {
                    Ok(text) => this.content = Some(text),
                    Err(e) => this.error = e.message,
                }
                cx.notify();
            },
        );
        Self {
            asset,
            content: None,
            error: String::new(),
        }
    }

    pub fn asset_id(&self) -> &str {
        &self.asset.id
    }
}

impl Render for AssetPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = self
            .content
            .as_deref()
            .map(text_sections)
            .unwrap_or_default();
        let name = self.asset.display_name.clone();
        let mut card = forms::card("asset-preview", &format!("Preview of {name}"), cx).child(
            Button::new("close-preview")
                .ghost()
                .label("Close preview")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(PreviewEvent::Close))),
        );
        if !self.error.is_empty() {
            card = card.child(widgets::text("preview-error", self.error.clone()));
        } else if self.content.is_none() {
            card = card.child(widgets::text("preview-loading", "Loading…"));
        }
        if sections.len() > 1 {
            card = card.child(h_flex().gap_2().flex_wrap().children(
                sections.iter().enumerate().map(|(i, s)| {
                    widgets::text(
                        eid(format!("preview-nav:{i}")),
                        if s.title.is_empty() { "Untitled".to_string() } else { s.title.clone() },
                    )
                }),
            ));
        }
        for (i, s) in sections.iter().enumerate() {
            card = card.child(
                v_flex()
                    .gap_1()
                    .when(!s.title.is_empty(), |el| {
                        el.child(widgets::styled_text(
                            eid(format!("preview-title:{i}")),
                            s.title.clone(),
                            |d| d.font_weight(FontWeight::MEDIUM),
                        ))
                    })
                    .child(widgets::styled_text(
                        eid(format!("preview-section:{i}")),
                        s.text.clone(),
                        |d| d.font_family("monospace").text_sm(),
                    )),
            );
        }
        card
    }
}
