//! Image asset picker for the image control (port of `AssetPicker.tsx`): the application's
//! image assets with a preview. The stored value stays the stable asset id.
//!
//! Test ids: `prop-asset` ("Image asset": `None`, then each image asset by name),
//! `asset-hint` (no image assets yet), `image-preview` ("Image preview") with the picture
//! or the status line `image-preview-state` ("Loading image…", "Image unavailable").

use super::studio::DesignStudio;
use crate::design::Control;
use crate::ui::{prelude::*, runtime::body::decode_data_url};
use std::sync::Arc;

pub(super) enum Preview {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

impl DesignStudio {
    /// The preview of an asset, started on first use.
    fn asset_preview(&mut self, id: &str, cx: &mut Context<Self>) -> &Preview {
        if !self.previews.contains_key(id) {
            self.previews.insert(id.to_string(), Preview::Loading);
            let (label, asset) = (self.shell.read(cx).label().to_string(), id.to_string());
            let key = id.to_string();
            task::spawn_call(
                cx,
                move || crate::asset_data::read_asset_data_url(label, asset),
                move |this, result, cx| {
                    let preview = match result.map_err(|e| e.message).and_then(|url| decode_data_url(&url)) {
                        Ok(image) => Preview::Ready(image),
                        Err(_) => Preview::Failed,
                    };
                    this.previews.insert(key, preview);
                    cx.notify();
                },
            );
        }
        &self.previews[id]
    }

    pub(super) fn render_asset_picker(&mut self, control: &Control, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let assets = self.image_assets(cx);
        let current = control.asset_id.clone().unwrap_or_default();
        let images: Vec<(String, String)> = assets
            .iter()
            .flatten()
            .filter(|a| a.media_type.starts_with("image/"))
            .map(|a| (a.id.clone(), a.display_name.clone()))
            .collect();
        let mut choices = vec![(String::new(), "None".to_string())];
        if !current.is_empty() && assets.is_some() && !images.iter().any(|(id, _)| *id == current) {
            choices.push((current.clone(), format!("Missing asset ({current})")));
        }
        let none_yet = assets.is_some() && images.is_empty();
        choices.extend(images);
        let cid = control.id.clone();
        let picker = self.pick(
            "prop-asset",
            "Image asset",
            &current,
            choices,
            "None",
            edit_pick!(cid, "Pick image", |c, v| c.asset_id = (!v.is_empty()).then_some(v)),
            cx,
        );
        let preview = (!current.is_empty()).then(|| {
            let shown = match self.asset_preview(&current, cx) {
                Preview::Ready(image) => img(image.clone()).max_h(px(160.)).into_any_element(),
                Preview::Loading => widgets::styled_text("image-preview-state", "Loading image…", move |d| {
                    d.ty(Type::Caption).text_color(muted)
                })
                .into_any_element(),
                Preview::Failed => widgets::styled_text("image-preview-state", "Image unavailable", move |d| {
                    d.ty(Type::Caption).text_color(muted)
                })
                .into_any_element(),
            };
            div()
                .id("image-preview")
                .aria_label("Image preview")
                .p_2()
                .border_1()
                .border_color(border)
                .rounded_md()
                .child(shown)
                .test_support()
        });
        v_flex()
            .gap_1()
            .child(picker)
            .when(none_yet, |el| {
                el.child(widgets::styled_text(
                    "asset-hint",
                    "Add image assets in App settings to use them here.",
                    move |d| d.ty(Type::Caption).text_color(muted),
                ))
            })
            .children(preview)
    }
}
