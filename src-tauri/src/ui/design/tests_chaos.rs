//! Chaos probes for the form designer. Each probe drives the real studio and checks an
//! invariant that a user would expect to hold. Exploratory: not wired into CI.

use super::test_support::{choose, control, form, id, open_designer, studio};
use super::tests::tall_document;
use crate::design::{ControlKind, Form};
use crate::ui::{shell::ModeId, testing::Harness};
use gpui_kit::{TestAppContext, point, px};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Orphans: a control whose container or tab page does not exist (it is never drawn).
fn orphans(form: &Form) -> Vec<String> {
    form.controls
        .iter()
        .filter(|c| {
            c.parent.as_ref().is_some_and(|p| {
                let Some(container) = form.controls.iter().find(|x| x.id == p.id) else {
                    return true;
                };
                match &p.tab {
                    Some(tab) => !container.tabs.iter().any(|t| &t.id == tab),
                    None => container.kind != ControlKind::Section,
                }
            })
        })
        .map(|c| format!("{} ({:?})", c.label, c.kind))
        .collect()
}

fn check(h: &mut Harness, step: &str) {
    let config = h.config();
    config.design.validate().unwrap_or_else(|e| panic!("{step}: stored design is invalid: {e}"));
    for f in &config.design.forms {
        let lost = orphans(f);
        assert!(lost.is_empty(), "{step}: orphaned controls on {}: {lost:?}", f.name);
    }
}

fn add_n(h: &mut Harness, kind: &str, n: usize) {
    for _ in 0..n {
        h.click(id(&format!("add-{kind}")));
    }
}

// ---- Undo granularity ----------------------------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_rapid_adds_are_separate_undo_steps(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    add_n(&mut h, "text", 3);
    assert_eq!(form(&h).controls.len(), 3);
    h.click("undo");
    assert_eq!(form(&h).controls.len(), 2, "one Undo should remove only the last added control");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_rapid_deletes_are_separate_undo_steps(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    add_n(&mut h, "text", 3);
    let ids: Vec<String> = form(&h).controls.iter().map(|c| c.id.clone()).collect();
    for cid in &ids {
        h.click(id(cid));
        h.click("delete-control");
    }
    assert!(form(&h).controls.is_empty());
    h.click("undo");
    assert_eq!(form(&h).controls.len(), 1, "one Undo should restore only the last deleted control");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_label_edits_on_two_controls_are_separate_undo_steps(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    add_n(&mut h, "text", 2);
    let ids: Vec<String> = form(&h).controls.iter().map(|c| c.id.clone()).collect();
    h.click(id(&ids[0]));
    h.fill("prop-label", "First");
    h.click(id(&ids[1]));
    h.fill("prop-label", "Second");
    h.click("undo");
    let labels: Vec<String> = form(&h).controls.iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels[0], "First", "undoing the second control's rename also reverted the first: {labels:?}");
}

// ---- Tabs ----------------------------------------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_add_after_removing_the_active_tab_page(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-tabs");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    // Make the second page active, then remove it.
    h.click(format!("tab-page:{}:{}", tabs.id, tabs.tabs[1].id));
    let s = studio(&mut h);
    let tabs_id = tabs.id.clone();
    h.cx.update(|cx| s.update(cx, |s, cx| s.select(Some(tabs_id), cx)));
    h.settle();
    h.click("remove-tab-2");
    h.click("add-label");
    check(&mut h, "add after removing the active tab");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_undo_add_tab_then_add_control(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-tabs");
    h.click("add-tab");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    h.click(format!("tab-page:{}:{}", tabs.id, tabs.tabs[2].id));
    h.click("undo");
    let s = studio(&mut h);
    let tabs_id = tabs.id.clone();
    h.cx.update(|cx| s.update(cx, |s, cx| s.select(Some(tabs_id), cx)));
    h.settle();
    h.click("add-text");
    check(&mut h, "add after undoing the active tab");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_remove_every_tab_page(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-tabs");
    h.click("remove-tab-2");
    h.click("remove-tab-1");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    eprintln!("CHAOS tabs after removing all pages: {}", tabs.tabs.len());
    h.click("add-text");
    check(&mut h, "add into a tab group with no pages");
}

// ---- Validation inputs ---------------------------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_min_above_max(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-number");
    h.fill("prop-min", "50");
    h.fill("prop-max", "100");
    let v = control(&h, ControlKind::Number).expect("number").validation;
    eprintln!("CHAOS min/max stored after typing 50/100: {:?}/{:?}", v.min, v.max);
    eprintln!("CHAOS error banner: {:?}", h.error());
    assert_eq!((v.min, v.max), (Some(50.0), Some(100.0)), "typing a max of 100 over a min of 50 lost the value");
    assert_eq!(h.error(), None, "transient keystrokes raised an error banner");
}

fn timed(h: &mut Harness, what: &str, f: impl FnOnce(&mut Harness)) {
    let t = std::time::Instant::now();
    f(h);
    eprintln!("CHAOS {what}: {:?}", t.elapsed());
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_hostile_form_names(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    for (n, name) in ["", "   ", "\u{202e}evil\u{202c}", "名前👍🏽\u{0}", "a\nb\tc"].into_iter().enumerate() {
        timed(&mut h, &format!("form name #{n}"), |h| { h.fill("form-name", name); h.press("enter"); });
        check(&mut h, "form name");
        eprintln!("CHAOS stored name #{n}: {:?} error {:?}", form(&h).name, h.error());
    }
    for len in [500usize, 2_000, 8_000] {
        let big = "x".repeat(len);
        timed(&mut h, &format!("form name len {len}"), |h| { h.fill("form-name", &big); h.press("enter"); });
    }
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_hostile_expressions(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-computed");
    for e in ["((((", "", "@@@", "\"unterminated", "[missing field]"] {
        timed(&mut h, &format!("expr {e:?}"), |h| { h.fill("prop-computed", e); h.press("enter"); });
        check(&mut h, "computed expression");
    }
    eprintln!("CHAOS stored computed: {:?}", control(&h, ControlKind::Computed).and_then(|c| c.computed));
    for depth in [50usize, 150, 1000] {
        let deep = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        timed(&mut h, &format!("expr depth {depth}"), |h| { h.fill("prop-computed", &deep); h.press("enter"); });
    }
    for len in [1_000usize, 5_000, 12_000] {
        let long = "1+".repeat(len / 2) + "1";
        timed(&mut h, &format!("expr len {len}"), |h| { h.fill("prop-computed", &long); h.press("enter"); });
    }
    check(&mut h, "after long expressions");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_hostile_label_lengths(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-text");
    for len in [500usize, 2_000, 8_000] {
        let big = "y".repeat(len);
        timed(&mut h, &format!("label len {len}"), |h| h.fill("prop-label", &big));
        timed(&mut h, &format!("rerender len {len}"), |h| h.settle());
    }
}

// ---- Schema changes under a bound form -----------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_bound_table_dropped_and_column_renamed_mid_edit(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("items", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("qty", "INTEGER", 0)]);
    open_designer(&mut h);
    choose(&mut h, "data-table", "items");
    h.click("add-text");
    super::test_support::wait_for_columns(&mut h, "items");
    choose(&mut h, "bound-column", "name");
    crate::alter_database_table(
        h.label.clone(),
        "items".into(),
        crate::data::AlterTable::RenameColumn { column: "name".into(), new_name: "title".into() },
    )
    .expect("rename");
    h.database_changed();
    h.settle();
    let binding = control(&h, ControlKind::Text).and_then(|c| c.binding).map(|b| b.column);
    eprintln!("CHAOS binding after column rename (backend path): {binding:?}");
    let cols = super::test_support::options_of(&mut h, "bound-column");
    eprintln!("CHAOS bound-column options after rename: {cols:?}");
    h.click("tab-preview");
    h.click("tab-form");
    crate::recordstore::commands::drop_database_table(h.label.clone(), "items".into()).expect("drop");
    h.database_changed();
    h.settle();
    h.click("tab-preview");
    eprintln!("CHAOS preview error after drop: {:?} / {:?}", h.error(), h.try_label("design-preview"));
    h.click("tab-form");
    h.click("add-number");
    check(&mut h, "after dropping the bound table");
    let issues = crate::design::validate_design(h.label.clone()).expect("issues");
    eprintln!("CHAOS problems after drop: {:?}", issues.iter().map(|i| i.message.clone()).collect::<Vec<_>>());
}

// ---- Form lifecycle ------------------------------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_delete_and_create_forms_keeps_names_unique(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("new-form");
    h.click("new-form");
    let second = h.config().design.forms[1].id.clone();
    super::test_support::open_form(&mut h, &second);
    h.settle();
    h.click("delete-form");
    h.click("new-form");
    let names: Vec<String> = h.config().design.forms.iter().map(|f| f.name.clone()).collect();
    eprintln!("CHAOS form names: {names:?}");
    let mut dedup = names.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), names.len(), "duplicate default form names: {names:?}");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_delete_open_form_while_previewing_then_undo_redo_storm(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("new-form");
    add_n(&mut h, "text", 2);
    h.click("tab-preview");
    h.click("tab-form");
    h.click("delete-form");
    for i in 0..30 {
        h.click(if i % 3 == 2 { "redo" } else { "undo" });
        check(&mut h, "undo/redo storm");
    }
    for _ in 0..40 {
        if !h.can_redo() {
            break;
        }
        h.click("redo");
    }
    check(&mut h, "after redo all");
}

// ---- Save, reload, mode switching ----------------------------------------------------------

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_save_reload_round_trip_after_chaos(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.fill("form-name", "名前 \u{202e}x");
    h.press("enter");
    h.click("add-section");
    h.click("add-text");
    h.click("form-breadcrumb-0");
    h.click("add-tabs");
    h.click("add-tab");
    let before = h.config().design;
    let path = h.temp_path("chaos.ixt");
    h.dialogs.queue_save(Some(path.clone()));
    h.click("save-project");
    let label = format!("{}-reopen", h.label);
    crate::open_document(label.clone(), path.to_string_lossy().into()).expect("reopen");
    let after = crate::read_document_config(label.clone()).expect("config").design;
    crate::close_document(label, true).ok();
    assert_eq!(serde_json::to_value(&before).unwrap(), serde_json::to_value(&after).unwrap(), "design changed across save/reload");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_switch_modes_mid_typing(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-text");
    h.type_into("prop-label", " half-typed");
    for mode in [ModeId::Data, ModeId::Design, ModeId::Automation, ModeId::Design] {
        h.switch_mode(mode);
    }
    eprintln!("CHAOS label after mode switching: {:?}", control(&h, ControlKind::Text).map(|c| c.label));
    h.press("ctrl-z");
    h.input("zzz");
    eprintln!("CHAOS label after keys post-switch: {:?}", control(&h, ControlKind::Text).map(|c| c.label));
    check(&mut h, "mode switch");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_typing_continues_into_a_deleted_control(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    add_n(&mut h, "text", 2);
    let ids: Vec<String> = form(&h).controls.iter().map(|c| c.id.clone()).collect();
    h.click(id(&ids[1]));
    h.type_into("prop-label", "A");
    // Undo from the toolbar while the label input keeps focus.
    let s = studio(&mut h);
    let store = h.read_shell(|s, _| s.store.clone());
    h.cx.update(|cx| store.update(cx, |s, cx| s.undo(cx)).ok());
    h.settle();
    h.input("B");
    let labels: Vec<(String, String)> = form(&h).controls.iter().map(|c| (c.id.clone(), c.label.clone())).collect();
    eprintln!("CHAOS labels after typing across undo: {labels:?}; selected {:?}", h.cx.read(|cx| s.read(cx).selected.clone()));
    check(&mut h, "typing across undo");
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_tiny_and_huge_windows(cx: &mut TestAppContext) {
    let mut h = Harness::start_sized(cx, (700.0, 420.0));
    h.click("new-document");
    h.switch_mode(ModeId::Design);
    h.settle();
    eprintln!("CHAOS tiny window: studio exists {}", h.exists("design-studio"));
    let design = crate::ui::pages::test_support::design_mode(&mut h);
    let s = h.cx.read(|cx| design.read(cx).forms.clone());
    for _ in 0..30 {
        h.cx.update(|cx| s.update(cx, |s, cx| s.add(ControlKind::Text, cx)));
    }
    h.settle();
    check(&mut h, "tiny window");
}

// ---- Monkey --------------------------------------------------------------------------------

fn raw_click(h: &mut Harness, x: f32, y: f32) {
    use gpui_kit::test::TestWindowExt as _;
    let position = point(px(x), px(y));
    h.update(|w, cx| w.drag(position, position, cx));
    h.settle();
}

fn monkey(cx: &mut TestAppContext, seed: u64, steps: usize) {
    use gpui_kit::test::TestWindowExt as _;
    let mut h = tall_document(cx);
    h.create_table("people", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("age", "INTEGER", 0)]);
    open_designer(&mut h);
    let mut rng = Rng(seed);
    let keys = ["alt-left", "alt-right", "alt-up", "alt-down", "alt-shift-left", "alt-shift-right", "alt-shift-down", "enter", "escape", "tab", "ctrl-z", "ctrl-shift-z", "delete", "backspace"];
    let kinds = ["text", "number", "section", "tabs", "label", "computed", "boolean", "related-list", "button"];
    let bounds = h.find("design-studio").bounds();
    let (w, hgt) = (f32::from(bounds.size.width), f32::from(bounds.size.height).min(1400.0));
    let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let mut log = vec![];
    for step in 0..steps {
        let action = rng.below(10);
        let what = match action {
            0..=3 => {
                let (x, y) = (ox + rng.below(w as u64) as f32, oy + rng.below(hgt as u64) as f32);
                raw_click(&mut h, x, y);
                format!("click {x},{y}")
            }
            4 => {
                let mut p = || point(px(ox + rng.below(w as u64) as f32), px(oy + rng.below(hgt as u64) as f32));
                let (a, b) = (p(), p());
                h.update(|win, cx| win.drag(a, b, cx));
                h.settle();
                format!("drag {a:?}->{b:?}")
            }
            5 => {
                let k = keys[rng.below(keys.len() as u64) as usize];
                h.press(k);
                format!("key {k}")
            }
            6 => {
                let k = kinds[rng.below(kinds.len() as u64) as usize];
                if h.exists(id(&format!("add-{k}"))) {
                    h.click(id(&format!("add-{k}")));
                }
                format!("add {k}")
            }
            7 => {
                let t = ["", "x", "名前", "((", "[name] > 1", &"z".repeat(300)][rng.below(6) as usize].to_string();
                h.input(&t);
                format!("input {:.20}", t)
            }
            8 => {
                let b = if rng.below(2) == 0 { "undo" } else { "redo" };
                if h.exists(b) {
                    h.click(b);
                }
                format!("{b}")
            }
            _ => {
                if h.exists("delete-control") {
                    h.click("delete-control");
                }
                "delete".to_string()
            }
        };
        if std::env::var("CHAOS_TRACE").is_ok() { eprintln!("CHAOS step {step}: {what}"); }
        log.push(what);
        let tail = log.iter().rev().take(12).rev().cloned().collect::<Vec<_>>().join(" | ");
        if !h.has_shell() {
            eprintln!("CHAOS seed {seed}: left the shell at step {step}; last: {tail}");
            return;
        }
        let config = h.config();
        if let Err(e) = config.design.validate() {
            panic!("seed {seed} step {step}: invalid design: {e}\nlast: {tail}");
        }
        for f in &config.design.forms {
            let lost = orphans(f);
            assert!(lost.is_empty(), "seed {seed} step {step}: orphans {lost:?}\nlast: {tail}");
        }
        if !h.has_shell() {
            eprintln!("CHAOS seed {seed}: left the shell at step {step}; last: {tail}");
            return;
        }
        if h.mode() != ModeId::Design {
            h.switch_mode(ModeId::Design);
        }
    }
    eprintln!("CHAOS seed {seed}: {steps} steps ok; error banner: {:?}; forms {}", h.error(), h.config().design.forms.len());
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_monkey_1(cx: &mut TestAppContext) {
    monkey(cx, 0x9E37_79B9_7F4A_7C15, 250);
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_monkey_2(cx: &mut TestAppContext) {
    monkey(cx, 0xD1B5_4A32_D192_ED03, 250);
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_monkey_3(cx: &mut TestAppContext) {
    monkey(cx, 42, 250);
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_monkey_4(cx: &mut TestAppContext) {
    monkey(cx, 7, 400);
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_monkey_5(cx: &mut TestAppContext) {
    monkey(cx, 0xABCDEF, 400);
}

#[gpui_kit::test]
#[ignore = "chaos probe: run with --ignored"]
fn probe_keystroke_cost(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-text");
    let n = 100;
    let label = h.label.clone();
    let config = h.config();
    let t = std::time::Instant::now();
    for _ in 0..n {
        crate::manager().unwrap().update_config(&label, config.clone()).unwrap();
    }
    eprintln!("CHAOS backend update_config x{n}: {:?}", t.elapsed());
    let store = h.read_shell(|s, _| s.store.clone());
    let t = std::time::Instant::now();
    for i in 0..n {
        h.cx.update(|cx| store.update(cx, |s, cx| s.update("Edit control", |c| c.design.forms[0].name = format!("n{i}"), cx)).unwrap());
    }
    eprintln!("CHAOS ConfigStore::update x{n} (no frame): {:?}", t.elapsed());
    let t = std::time::Instant::now();
    h.settle();
    eprintln!("CHAOS settle after store storm: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    h.type_into("prop-label", &"k".repeat(n));
    eprintln!("CHAOS typing {n} chars into prop-label: {:?}", t.elapsed());
    h.click("tab-preview");
    let t = std::time::Instant::now();
    h.update(|w, cx| { use gpui_kit::test::TestWindowExt as _; for _ in 0..n { w.render_frame(cx); } });
    eprintln!("CHAOS {n} frames of the preview: {:?}", t.elapsed());
}

