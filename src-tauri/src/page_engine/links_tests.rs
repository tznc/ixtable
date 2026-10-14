use super::*;
use serde_json::json;

fn config() -> DocumentConfig {
    let mut config = DocumentConfig::default();
    config.pages = serde_json::from_value(json!([
        {"id": "orders", "name": "Orders", "parameters": [
            {"name": "region", "logicalType": "text", "required": true},
            {"name": "limit", "logicalType": "integer", "defaultValue": 10}
        ]},
        {"id": "home", "name": "Home", "components": [
            {"id": "b1", "kind": "button", "title": "Go", "label": "East orders", "targetPageId": "orders",
             "pageParams": {"region": "'East'"}},
            {"id": "b2", "kind": "button", "title": "Broken", "targetPageId": "orders"}
        ]}
    ]))
    .unwrap();
    config.design.navigation = serde_json::from_value(json!([
        {"id": "n1", "label": "West", "kind": "page", "targetId": "orders", "params": {"region": "'West'", "colour": "'red'"}}
    ]))
    .unwrap();
    config.actions = serde_json::from_value(json!([
        {"id": "a1", "name": "Drill", "steps": [
            {"id": "s1", "kind": "condition", "when": "true",
             "then": [{"id": "s2", "kind": "openDashboard", "dashboardId": "orders", "params": {"region": "params.region"}}],
             "else": []}
        ]}
    ]))
    .unwrap();
    config
}

#[test]
fn lists_navigation_button_and_step_links_with_their_parameters() {
    let config = config();
    let links: Vec<String> = page_links(&config).iter().map(|l| l.describe_with(&config.pages)).collect();
    assert_eq!(
        links,
        [
            "Navigation item West opens Orders (colour = 'red', region = 'West')",
            "Button East orders on Home opens Orders (region = 'East')",
            "Button Broken on Home opens Orders",
            "Action Drill opens Orders (region = params.region)",
        ]
    );
}

#[test]
fn reports_missing_required_and_unknown_parameters() {
    let issues = link_issues(&config());
    let messages: Vec<&str> = issues.iter().map(|i| i.message.as_str()).collect();
    assert_eq!(issues.len(), 2, "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("Button Broken") && m.contains("required parameter region has no value")));
    assert!(messages.iter().any(|m| m.contains("Orders has no parameter colour")));
}

#[test]
fn checks_parameter_names() {
    let mut config = config();
    config.pages[0].parameters.push(crate::archive::QueryParameter { name: "region".into(), ..Default::default() });
    config.pages[0].parameters.push(crate::archive::QueryParameter { name: "bad name".into(), ..Default::default() });
    let messages: Vec<String> = link_issues(&config).into_iter().map(|i| i.message).collect();
    assert!(messages.iter().any(|m| m.contains("parameter region is declared twice")));
    assert!(messages.iter().any(|m| m.contains("\"bad name\"")));
}

#[test]
fn evaluates_values_and_fills_defaults() {
    let config = config();
    let mut scope = crate::expr::Scope::new();
    scope.insert("params".into(), json!({"region": "North"}));
    let params = evaluate_params(&[("region".to_string(), "params.region".to_string())].into(), &scope);
    let params = with_defaults(&config.pages[0], params);
    assert_eq!(serde_json::Value::Object(params), json!({"region": "North", "limit": 10}));
}

#[test]
fn navigation_items_pass_their_parameters_to_the_runtime_page() {
    let config = config();
    let page = crate::forms::navigation::page_for(&config.design.navigation[0]).unwrap();
    assert_eq!(page.params.get("region"), Some(&json!("West")));
}
