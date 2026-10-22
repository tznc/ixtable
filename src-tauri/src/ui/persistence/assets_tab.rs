//! Application assets (PRD 18): import, export, remove, unused-asset cleanup, archive
//! size and checkpoints. Port of src/persistence/AssetsTab.tsx.
//!
//! Test ids: `import-asset`, `asset-count`, `asset-error`,
//! `asset-row:<name>` (label "name type size"), `view-asset:<name>`,
//! `export-asset:<name>`, `remove-asset:<name>`, badge `asset-unused:<name>` on
//! unreferenced assets, card `unused-assets`,
//! `unused-assets-note`, `unused-asset:<i>`, `remove-unused-assets`. Children: `ArchiveSizePanel`,
//! `CheckpointsPanel`, `AssetPreview`.

use super::{
    archive_size::ArchiveSizePanel,
    asset_preview::{AssetPreview, PreviewEvent},
    checkpoints::CheckpointsPanel,
    eid,
    format::{format_bytes, plural},
    text_sections::is_text_asset,
};
use crate::{
    archive::Attachment,
    manager::AppError,
    ui::{
        dialogs,
        display::{Tone, status_badge},
        forms,
        prelude::*,
        shell::Shell,
        store::StoreEvent,
        toast::Toast,
    },
};

pub struct AssetsTab {
    shell: Entity<Shell>,
    assets: Option<Vec<Attachment>>,
    orphans: Vec<Attachment>,
    busy: bool,
    error: Option<AppError>,
    viewing: Option<Entity<AssetPreview>>,
    size: Entity<ArchiveSizePanel>,
    checkpoints: Entity<CheckpointsPanel>,
    _subscriptions: Vec<Subscription>,
}

impl AssetsTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let label = shell.read(cx).label().to_string();
        let store = shell.read(cx).store.clone();
        let size = cx.new(|cx| ArchiveSizePanel::new(label, cx));
        let checkpoints = cx.new(|cx| CheckpointsPanel::new(shell.clone(), cx));
        let subscriptions = vec![cx.subscribe(&store, |this, _, e: &StoreEvent, cx| {
            if matches!(e, StoreEvent::Changed) {
                this.refresh(cx);
            }
        })];
        let mut this = Self {
            shell,
            assets: None,
            orphans: vec![],
            busy: false,
            error: None,
            viewing: None,
            size,
            checkpoints,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || {
                let all = crate::list_attachments(label.clone())?;
                let unused = crate::assets::list_orphan_assets(label)?;
                Ok::<_, AppError>((all, unused))
            },
            |this, result, cx| {
                match result {
                    Ok((all, unused)) => {
                        this.assets = Some(all);
                        this.orphans = unused;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn begin(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
    }

    /// Ends an action: shows its message or error, then re-reads the lists and the size.
    fn finish(&mut self, result: Result<String, AppError>, cx: &mut Context<Self>) {
        self.busy = false;
        match result {
            Ok(message) => {
                let toast = if message.ends_with("canceled.") { Toast::info(message) } else { Toast::success(message) };
                self.shell.update(cx, |shell, cx| shell.toast(toast, cx));
            }
            Err(e) => self.error = Some(e),
        }
        self.refresh(cx);
        self.size.update(cx, |s, cx| s.measure(cx));
        cx.notify();
    }

    fn adopt(&mut self, state: &crate::manager::SessionState, cx: &mut Context<Self>) {
        let shell = self.shell.clone();
        shell.update(cx, |s, cx| s.apply_session(state, cx));
    }

    fn import_one(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let pick = dialogs::pick_file(cx, "Import application asset", &[]);
        task::after(cx, pick, |this, path, cx| {
            let Some(path) = path else {
                return this.finish(Ok("Import canceled.".into()), cx);
            };
            let label = this.label(cx);
            let path = path.to_string_lossy().into_owned();
            task::spawn_call(
                cx,
                move || crate::assets::import_asset(label, path, None),
                |this, result, cx| match result {
                    Ok(import) => {
                        this.adopt(&import.state, cx);
                        let name = import.asset.display_name;
                        this.finish(
                            Ok(if import.deduplicated {
                                format!("Same content as {name}; reused the existing asset.")
                            } else {
                                format!("Imported {name}.")
                            }),
                            cx,
                        );
                    }
                    Err(e) => this.finish(Err(e), cx),
                },
            );
        });
    }

    fn export_one(&mut self, asset: Attachment, cx: &mut Context<Self>) {
        self.begin(cx);
        let pick = dialogs::save_file(cx, "Export application asset", &asset.display_name, &[]);
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else {
                return this.finish(Ok("Export canceled.".into()), cx);
            };
            let label = this.label(cx);
            let (id, name) = (asset.id.clone(), asset.display_name.clone());
            let dest = path.to_string_lossy().into_owned();
            task::spawn_call(
                cx,
                move || crate::export_attachment(label, id, dest),
                move |this, result, cx| {
                    this.finish(result.map(|_| format!("Exported {name}.")), cx)
                },
            );
        });
    }

    fn remove_one(&mut self, asset: Attachment, cx: &mut Context<Self>) {
        self.begin(cx);
        let ask = dialogs::confirm(
            cx,
            "Remove asset",
            &format!("Remove {} from this application?", asset.display_name),
        );
        task::after(cx, ask, move |this, ok, cx| {
            if !ok {
                return this.finish(Ok(String::new()), cx);
            }
            let label = this.label(cx);
            let (id, name) = (asset.id.clone(), asset.display_name.clone());
            task::spawn_call(
                cx,
                move || crate::remove_attachment(label, id),
                move |this, result, cx| match result {
                    Ok(state) => {
                        this.adopt(&state, cx);
                        this.finish(Ok(format!("Removed {name}.")), cx);
                    }
                    Err(e) => this.finish(Err(e), cx),
                },
            );
        });
    }

    fn cleanup(&mut self, cx: &mut Context<Self>) {
        self.begin(cx);
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::assets::cleanup_orphan_assets(label),
            |this, result, cx| match result {
                Ok(cleanup) => {
                    this.adopt(&cleanup.state, cx);
                    this.finish(
                        Ok(format!("Removed {}.", plural(cleanup.removed.len(), "unused asset"))),
                        cx,
                    );
                }
                Err(e) => this.finish(Err(e), cx),
            },
        );
    }

    fn toggle_preview(&mut self, asset: Attachment, cx: &mut Context<Self>) {
        if self.viewing.as_ref().is_some_and(|v| v.read(cx).asset_id() == asset.id) {
            self.viewing = None;
        } else {
            let label = self.label(cx);
            let view = cx.new(|cx| AssetPreview::new(label, asset, cx));
            cx.subscribe(&view, |this, _, _: &PreviewEvent, cx| {
                this.viewing = None;
                cx.notify();
            })
            .detach();
            self.viewing = Some(view);
        }
        cx.notify();
    }
}

impl Render for AssetsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        let assets = self.assets.clone().unwrap_or_default();
        let viewing = self
            .viewing
            .clone()
            .filter(|v| assets.iter().any(|a| a.id == v.read(cx).asset_id()));
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("assets-heading", "Assets"))
            .child(forms::muted(
                "Files stored inside this application's archive, such as logos and report images. \
                 Identical files are stored once.",
                cx,
            ))
            .child(
                forms::actions()
                    .child(
                        forms::primary("import-asset", "Import asset")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.import_one(cx))),
                    )
                    .child(widgets::text(
                        "asset-count",
                        self.assets
                            .as_ref()
                            .map_or("Loading assets…".to_string(), |a| plural(a.len(), "asset")),
                    )),
            );
        if let Some(e) = &self.error {
            panel = panel.child(widgets::error_banner("asset-error", e, None, cx));
        }
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        if !assets.is_empty() {
            panel = panel.child(
                h_flex()
                    .gap_3()
                    .px_2()
                    .pb_1()
                    .border_b_1()
                    .border_color(border)
                    .ty(Type::Caption)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(muted)
                    .child(div().flex_1().child("Name"))
                    .child(div().w(px(140.)).child("Type"))
                    .child(div().w(px(80.)).text_right().child("Size"))
                    .child(div().w(px(120.)).child("Checksum"))
                    .child(div().w(px(220.))),
            );
        }
        let mut rows = v_flex().ty(Type::Body);
        for asset in &assets {
            let name = asset.display_name.clone();
            let short_sum = asset.checksum.chars().take(12).collect::<String>();
            let unused = self.orphans.iter().any(|o| o.id == asset.id);
            let row = h_flex()
                .id(eid(format!("asset-row:{name}")))
                .aria_label(format!(
                    "{name} {} {} {short_sum}",
                    asset.media_type,
                    format_bytes(asset.size),
                ))
                .gap_3()
                .items_center()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name.clone()))
                        .when(unused, |el| {
                            el.child(status_badge(eid(format!("asset-unused:{name}")), "Unused", Tone::Warning, cx))
                        }),
                )
                .child(div().w(px(140.)).text_color(muted).child(asset.media_type.clone()))
                .child(div().w(px(80.)).text_right().child(format_bytes(asset.size)))
                .child(
                    div()
                        .w(px(120.))
                        .font_family("monospace")
                        .ty(Type::Caption)
                        .text_color(muted)
                        .child(format!("{short_sum}…")),
                )
                .child(
                    h_flex()
                        .w(px(220.))
                        .gap_1()
                        .justify_end()
                        .when(is_text_asset(&asset.media_type), |el| {
                            let a = asset.clone();
                            el.child(
                                forms::secondary(eid(format!("view-asset:{name}")), "View")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_preview(a.clone(), cx)
                                    })),
                            )
                        })
                        .child({
                            let a = asset.clone();
                            forms::secondary(eid(format!("export-asset:{name}")), "Export")
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| this.export_one(a.clone(), cx)))
                        })
                        .child({
                            let a = asset.clone();
                            forms::danger(eid(format!("remove-asset:{name}")), "Remove")
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| this.remove_one(a.clone(), cx)))
                        }),
                )
                .test_support();
            rows = rows.child(row);
        }
        panel = panel.child(rows);
        if let Some(v) = viewing {
            panel = panel.child(v);
        }
        let mut unused = forms::card("unused-assets", "Unused assets", cx);
        unused = if self.orphans.is_empty() {
            unused.child(widgets::text(
                "unused-assets-note",
                "Every asset is referenced by the application.",
            ))
        } else {
            let names: Vec<&str> = self.orphans.iter().map(|a| a.display_name.as_str()).collect();
            let count = plural(names.len(), "asset");
            unused
                .child(widgets::text(
                    "unused-assets-note",
                    format!("No form, report, or setting refers to {count}. They are marked Unused above."),
                ))
                .child(
                    v_flex()
                        .ty(Type::Body)
                        .children(names.iter().enumerate().map(|(i, n)| {
                            widgets::text(eid(format!("unused-asset:{i}")), n.to_string())
                        })),
                )
                .child(
                    h_flex().child(
                        forms::danger("remove-unused-assets", "Remove unused assets")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.cleanup(cx))),
                    ),
                )
        };
        panel
            .child(unused)
            .child(self.size.clone())
            .child(self.checkpoints.clone())
    }
}
