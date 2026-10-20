//! Start-screen layout: primary actions, the Access card, template and recent rows.

use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn keeps_new_and_open_primary_with_access_featured_and_the_bundle_as_a_link(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    for id in ["new-document", "open-document", "import-access", "open-bundle"] {
        assert!(h.exists(id), "{id}");
    }
    assert_eq!(h.label("import-access-title"), "Coming from Microsoft Access?");
}

#[gpui_kit::test]
fn lists_every_template_with_its_name(cx: &mut TestAppContext) {
    let mut h = Harness::start(cx);
    for id in ["crm", "inventory"] {
        assert!(h.exists(format!("template:{id}")), "{id}");
    }
}

#[test]
fn templates_get_distinct_icons_and_versions_share_one() {
    use super::template_icon;
    assert_ne!(template_icon("crm"), template_icon("inventory"));
    assert_eq!(template_icon("work-orders"), template_icon("work-orders@2"));
}
