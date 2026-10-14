//! Extra start-screen entry points owned by feature modules (Access import, runtime
//! bundles, cloud apps, crash recovery). A feature adds its button in `actions` (built
//! every render) or its panel view in `panels` (built once with the screen).
//!
//! A button and its panel talk through `Extras`, a per-screen registry of weak handles
//! (the screen owns the panel views; the registry never keeps them alive).

use super::{
    access::host::AccessHost,
    cloud::{
        cloud_apps::{CloudApps, CloudAppsEvent},
        open::{adopt_runtime, set_open_notice},
    },
    persistence::recovery_list::RecoveryList,
    release::bundle_flow::{Act, BundleEvent, BundleFlow},
    start::StartScreen,
};
use super::widgets::StartAligned as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Disableable as _, Icon,
    button::Button,
};
use gpui_kit::*;
use std::{collections::HashMap, sync::Arc};

/// Weak handles to the views a start screen's extras share.
#[derive(Default)]
pub struct ScreenExtras {
    pub bundle: Option<WeakEntity<BundleFlow>>,
    pub access: Option<WeakEntity<AccessHost>>,
}

#[derive(Default)]
struct Registry(HashMap<EntityId, ScreenExtras>);
impl Global for Registry {}

fn with_extras<R>(screen: EntityId, cx: &mut App, f: impl FnOnce(&mut ScreenExtras) -> R) -> R {
    if cx.try_global::<Registry>().is_none() {
        cx.set_global(Registry::default());
    }
    f(cx.global_mut::<Registry>().0.entry(screen).or_default())
}

fn bundle_flow(screen: EntityId, cx: &mut App) -> Option<Entity<BundleFlow>> {
    with_extras(screen, cx, |e| e.bundle.as_ref().and_then(WeakEntity::upgrade))
}

fn access_host(screen: EntityId, cx: &mut App) -> Option<Entity<AccessHost>> {
    with_extras(screen, cx, |e| e.access.as_ref().and_then(WeakEntity::upgrade))
}

/// Buttons next to New and Open.
pub fn actions(screen: &Entity<StartScreen>, busy: bool, _: &mut App) -> Vec<AnyElement> {
    let id = screen.entity_id();
    vec![
        Button::new("import-access")
            .start_label(Some(Icon::new(IconName::DatabaseZap)), "Import from Access…")
            .tooltip("Convert a Microsoft Access file into a new document")
            .outline()
            .disabled(busy)
            .w(px(240.))
            .on_click(move |_, _, cx| {
                if let Some(host) = access_host(id, cx) {
                    host.update(cx, |h, cx| h.open(cx));
                }
            })
            .into_any_element(),
        Button::new("open-bundle")
            .start_label(Some(Icon::new(IconName::PackageOpen)), "Open runtime bundle")
            .tooltip("Open a signed .ixtr bundle")
            .outline()
            .disabled(busy)
            .w(px(220.))
            .on_click(move |_, _, cx| {
                if let Some(flow) = bundle_flow(id, cx) {
                    flow.update(cx, |f, cx| f.start(cx));
                }
            })
            .into_any_element(),
    ]
}

/// Panels between the banners and the template list. `label` is the window label of
/// the screen (the screen itself is still being built and cannot be read).
pub fn panels(
    screen: &Entity<StartScreen>,
    label: &str,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyView> {
    let label = label.to_string();
    let act: Act = {
        let label = label.clone();
        Arc::new(move |path, password, allow_downgrade, sha| {
            crate::installation_commands::open_runtime_bundle(
                label.clone(),
                path.to_string(),
                (!password.is_empty()).then(|| password.to_string()),
                Some(allow_downgrade),
                Some(sha.to_string()),
            )
        })
    };
    let flow = cx.new(|cx| BundleFlow::new(label.clone(), None, act, false, window, cx));
    let weak_screen = screen.downgrade();
    cx.subscribe(&flow, move |_, event: &BundleEvent, cx| {
        // A verified bundle opened: hand its session to the screen, which opens the shell.
        if let BundleEvent::Done(state, _) = event {
            let state = state.clone();
            let _ = weak_screen.update(cx, |screen, cx| {
                screen.run("Opening bundle…", move |_| Ok(state), cx)
            });
        }
    })
    .detach();
    with_extras(screen.entity_id(), cx, |e| e.bundle = Some(flow.downgrade()));
    let access = {
        let (label, weak) = (label.clone(), screen.downgrade());
        cx.new(|_| AccessHost::new(label, weak))
    };
    with_extras(screen.entity_id(), cx, |e| e.access = Some(access.downgrade()));
    let apps = cx.new(|cx| CloudApps::new(label.clone(), cx));
    let weak_screen = screen.downgrade();
    cx.subscribe(&apps, move |_, event: &CloudAppsEvent, cx| {
        let CloudAppsEvent::Opened(opened) = event;
        // The cloud installation takes effect (its role) and the opened window shows
        // what the update did.
        adopt_runtime(opened, cx);
        set_open_notice(&opened.notice, cx);
        let state = opened.state.clone();
        let _ = weak_screen.update(cx, |screen, cx| screen.run("Opening cloud app…", move |_| Ok(state), cx));
    })
    .detach();
    vec![
        flow.into(),
        access.into(),
        apps.into(),
        cx.new(|cx| RecoveryList::new(screen, cx)).into(),
    ]
}

/// Opens a runtime bundle (.ixtr) the OS asked to open.
pub fn open_bundle(screen: &mut StartScreen, path: String, cx: &mut Context<StartScreen>) {
    let _ = screen;
    match bundle_flow(cx.entity_id(), cx) {
        Some(flow) => flow.update(cx, |f, cx| f.verify(path, cx)),
        None => screen.set_notice("The bundle view is not ready yet; try again.", cx),
    }
}
