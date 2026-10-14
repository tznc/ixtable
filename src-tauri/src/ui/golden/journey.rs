//! Golden-app journeys (PRD §26.5), port of `tests/integration/golden/journey.tsx`: the
//! evidence recorder and the helpers the golden suites share. Every journey writes
//! `tests/golden-evidence/<app>-<journey>.json` (relative to the repository root) with its steps, assertions, timings and, on failure, the error, the steps that
//! reproduce it and the tail of the local diagnostic log.
//!
//! Deviation from the TypeScript evidence: `environment` carries `runtime: "rust"` and the
//! crate version instead of the Node version.

use crate::data::{DataValue, NamedValue};
use crate::ui::testing::{Harness, UiContext};
use gpui_kit::AppContext as _;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Where the evidence files go: `tests/golden-evidence` at the repository root.
pub fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tests")
        .join("golden-evidence")
}

struct Assertion {
    description: String,
    expected: Value,
    actual: Value,
    passed: bool,
}

struct Step {
    name: String,
    failed: bool,
    started_at: String,
    duration_ms: u64,
    assertions: Vec<Assertion>,
    error: Option<String>,
}

/// Large values (whole configs, tables) are recorded as a digest so evidence stays small.
fn summarize(value: Value) -> Value {
    let json = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    if json.len() <= 2000 {
        return value;
    }
    json!({"sha256": format!("{:x}", Sha256::digest(json.as_bytes())), "bytes": json.len()})
}

/// Integral floats become integers, so `2` and `2.0` compare equal like in JSON text.
pub fn normalize(value: Value) -> Value {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.as_i64().is_none() && f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(normalize).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "panic".into()
    }
}

/// Records one journey's steps and assertions and writes them as JSON evidence.
pub struct Journey {
    pub app: String,
    pub name: String,
    steps: Vec<Step>,
    current: Option<usize>,
    started: Instant,
    started_at: String,
}

impl Journey {
    pub fn new(app: &str, name: &str) -> Self {
        Self {
            app: app.into(),
            name: name.into(),
            steps: vec![],
            current: None,
            started: Instant::now(),
            started_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }
    }

    /// Runs one named user step; a failure is recorded before it propagates.
    pub fn step<T>(&mut self, name: &str, run: impl FnOnce(&mut Journey) -> T) -> T {
        self.steps.push(Step {
            name: name.into(),
            failed: false,
            started_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            duration_ms: 0,
            assertions: vec![],
            error: None,
        });
        let index = self.steps.len() - 1;
        self.current = Some(index);
        let t0 = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| run(self)));
        self.steps[index].duration_ms = t0.elapsed().as_millis() as u64;
        self.current = None;
        match result {
            Ok(value) => value,
            Err(payload) => {
                self.steps[index].failed = true;
                self.steps[index].error = Some(panic_message(&*payload));
                resume_unwind(payload)
            }
        }
    }

    /// Deep-equality assertion that is also written to the evidence.
    pub fn check(&mut self, description: &str, actual: impl serde::Serialize, expected: impl serde::Serialize) {
        let actual = normalize(serde_json::to_value(actual).expect("serializable actual"));
        let expected = normalize(serde_json::to_value(expected).expect("serializable expected"));
        let passed = actual == expected;
        let index = self.current.or(self.steps.len().checked_sub(1));
        if let Some(step) = index.and_then(|i| self.steps.get_mut(i)) {
            step.assertions.push(Assertion {
                description: description.into(),
                expected: summarize(expected.clone()),
                actual: summarize(actual.clone()),
                passed,
            });
        }
        assert_eq!(actual, expected, "{description}");
    }

    /// The evidence document; `error` is the failure of the journey itself, if any.
    fn evidence(&self, error: Option<&str>, log_tail: Option<Value>) -> Value {
        let failed = self.steps.iter().position(|s| s.failed);
        let mut doc = json!({
            "app": self.app,
            "journey": self.name,
            "status": if error.is_some() || failed.is_some() { "failed" } else { "passed" },
            "startedAt": self.started_at,
            "durationMs": self.started.elapsed().as_millis() as u64,
            "environment": {
                "platform": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "runtime": "rust",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "steps": self.steps.iter().map(|s| {
                let mut step = json!({
                    "name": s.name,
                    "status": if s.failed { "failed" } else { "passed" },
                    "startedAt": s.started_at,
                    "durationMs": s.duration_ms,
                    "assertions": s.assertions.iter().map(|a| json!({
                        "description": a.description,
                        "expected": a.expected,
                        "actual": a.actual,
                        "passed": a.passed,
                    })).collect::<Vec<_>>(),
                });
                if let Some(error) = &s.error {
                    step["error"] = json!(error);
                }
                step
            }).collect::<Vec<_>>(),
        });
        if error.is_some() || failed.is_some() {
            let message = failed
                .and_then(|i| self.steps[i].error.clone())
                .or_else(|| error.map(String::from))
                .unwrap_or_default();
            let upto = failed.map_or(self.steps.len(), |i| i + 1);
            doc["error"] = json!(message);
            doc["reproduction"] = json!(
                self.steps[..upto]
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("{}. {}", i + 1, s.name))
                    .collect::<Vec<_>>()
            );
            if let Some(tail) = log_tail {
                doc["logTail"] = tail;
            }
        }
        doc
    }

    /// Writes the evidence file; `with_journey` calls it on success and on failure.
    pub fn write(&self, error: Option<&str>, log_tail: Option<Value>) {
        let dir = evidence_dir();
        std::fs::create_dir_all(&dir).expect("create the evidence directory");
        let text = serde_json::to_string_pretty(&self.evidence(error, log_tail)).expect("evidence json");
        std::fs::write(dir.join(format!("{}-{}.json", self.app, self.name)), format!("{text}\n"))
            .expect("write the evidence");
    }
}

/// The newest local diagnostic log entries, or None when they cannot be read.
pub fn read_log_tail(label: &str, limit: usize) -> Option<Value> {
    let entries = crate::logging::read_logs(label.into(), Some(limit)).ok()?;
    serde_json::to_value(entries).ok()
}

/// Runs a journey and always writes its evidence.
pub fn with_journey(app: &str, name: &str, run: impl FnOnce(&mut Journey)) {
    let mut journey = Journey::new(app, name);
    match catch_unwind(AssertUnwindSafe(|| run(&mut journey))) {
        Ok(()) => journey.write(None, None),
        Err(payload) => {
            let message = panic_message(&*payload);
            journey.write(Some(&message), read_log_tail("main", 100));
            resume_unwind(payload)
        }
    }
}

// ---- data helpers ----------------------------------------------------------------------

/// A cell as plain JSON: null for NULL, else its value.
pub fn plain(cell: &DataValue) -> Value {
    serde_json::to_value(cell)
        .ok()
        .and_then(|v| v.get("value").cloned())
        .unwrap_or(Value::Null)
}

fn plain_rows(rows: &[Vec<DataValue>]) -> Vec<Vec<Value>> {
    rows.iter().map(|row| row.iter().map(plain).collect()).collect()
}

/// Plain rows of a read-only SQL query through DuckDB.
pub fn sql(label: &str, text: &str) -> Vec<Vec<Value>> {
    let result = crate::execute_read_query(label.into(), text.into()).expect("read query");
    plain_rows(&result.rows)
}

pub fn scalar(label: &str, text: &str) -> Value {
    sql(label, text).first().and_then(|r| r.first().cloned()).unwrap_or(Value::Null)
}

fn to_data_value(value: &Value) -> DataValue {
    match value {
        Value::Null => DataValue::Null,
        Value::Number(n) if n.is_i64() => DataValue::Integer(n.as_i64().unwrap_or(0)),
        Value::Number(n) => DataValue::Real(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => DataValue::Text(s.clone()),
        other => DataValue::Text(other.to_string()),
    }
}

/// Id of the saved query named `name`.
pub fn query_id(label: &str, name: &str) -> String {
    let config = crate::read_document_config(label.into()).expect("config");
    config
        .saved_queries
        .iter()
        .find(|q| q.name == name)
        .unwrap_or_else(|| panic!("No savedQueries entry named {name}"))
        .id
        .clone()
}

/// Runs a saved query (by name) through the public Queries command and returns plain rows.
pub fn try_saved_query(label: &str, name: &str, params: &[(&str, Value)]) -> Result<Vec<Vec<Value>>, crate::manager::AppError> {
    let params = params
        .iter()
        .map(|(column, value)| NamedValue { column: column.to_string(), value: to_data_value(value) })
        .collect();
    let run = tauri::async_runtime::block_on(crate::queries::run_saved_query(
        label.into(),
        query_id(label, name),
        params,
        None,
        None,
    ))?;
    Ok(plain_rows(&run.rows))
}

pub fn saved_query(label: &str, name: &str, params: &[(&str, Value)]) -> Vec<Vec<Value>> {
    try_saved_query(label, name, params).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// Validation errors of the open document as "kind id: message".
pub fn validation_errors(label: &str) -> Vec<String> {
    crate::validation::validate_document(label.into())
        .expect("validate")
        .into_iter()
        .filter(|i| matches!(i.severity, crate::validation::Severity::Error))
        .map(|i| format!("{} {}: {}", i.object_kind, i.object_id, i.message))
        .collect()
}

/// The error code of a failed backend call, or "OK".
pub fn code_of<T>(result: Result<T, crate::manager::AppError>) -> String {
    match result {
        Ok(_) => "OK".into(),
        Err(e) => e.code,
    }
}

// ---- UI helpers ------------------------------------------------------------------------

/// Opens the start screen and creates the document from a template through the UI.
pub fn start_from_template<'a, C: UiContext>(cx: &'a mut C, id: &str) -> Harness<'a, C> {
    // A tall window keeps every control of a long form (related lists, choices) on screen.
    let mut h = Harness::start_sized(cx, (1280.0, 2800.0));
    h.click(format!("template:{id}"));
    assert!(h.has_shell(), "the {id} template did not open");
    h.switch_mode(crate::ui::shell::ModeId::Run);
    wait(&mut h);
    h
}

/// Lets background work finish: the test clock advances in steps, with real time between
/// them for the threads that run actions.
pub fn wait<C: UiContext>(h: &mut Harness<C>) {
    for _ in 0..16 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

/// Polls until `done` holds (writes land through background threads).
pub fn eventually<C: UiContext>(h: &mut Harness<C>, what: &str, mut done: impl FnMut(&mut Harness<C>) -> bool) {
    for _ in 0..200 {
        h.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(10));
        h.settle();
        if done(h) {
            return;
        }
    }
    panic!("timed out waiting for {what}");
}

/// The id of the navigation button labelled `label`.
pub fn nav<C: UiContext>(h: &mut Harness<C>, label: &str) -> String {
    fn find<'a>(items: &'a [crate::design::NavigationItem], label: &str) -> Option<&'a crate::design::NavigationItem> {
        items.iter().find_map(|n| {
            if n.label == label && n.kind != crate::design::NavKind::Group {
                Some(n)
            } else {
                find(&n.children, label)
            }
        })
    }
    let config = h.config();
    let item = find(&config.design.navigation, label).unwrap_or_else(|| panic!("no navigation item {label}"));
    format!("nav:{}", item.id)
}

/// Clicks a Runtime navigation entry; choosing the entry that is already open returns to
/// its list too.
pub fn open_page<C: UiContext>(h: &mut Harness<C>, label: &str) {
    let id = nav(h, label);
    h.click(id);
    wait(h);
}

pub use open_page as show_list;

/// The index of the list row labelled "Open <name>".
pub fn row_of<C: UiContext>(h: &mut Harness<C>, name: &str) -> usize {
    (0..50)
        .find(|n| h.try_label(format!("row:{n}")).as_deref() == Some(&format!("Open {name}")))
        .unwrap_or_else(|| panic!("no list row for {name}"))
}

pub fn open_row<C: UiContext>(h: &mut Harness<C>, name: &str) {
    let n = row_of(h, name);
    h.click(format!("row:{n}"));
    wait(h);
}

/// Picks a relationship option once the lookup has loaded it.
pub fn choose_related<C: UiContext>(h: &mut Harness<C>, field: &str, option: &str) {
    h.click(format!("field:{field}"));
    wait(h);
    h.click(format!("field:{field}:option:{}", crate::ui::widgets::slug(option)));
    wait(h);
}

/// Like `Harness::fill`, inside the scope `scope` (an embedded form repeats the parent's ids).
pub fn fill_in<C: UiContext>(h: &mut Harness<C>, scope: &str, id: &str, text: &str) {
    h.click_in(scope.to_string(), id.to_string());
    h.select_all();
    h.press("backspace");
    if !text.is_empty() {
        h.input(text);
    }
}

/// The accessibility label of `id` inside `scope`, if present.
pub fn label_in<C: UiContext>(h: &mut Harness<C>, scope: &str, id: &str) -> Option<String> {
    use gpui_kit::test::TestWindowExt as _;
    let (scope, id) = (scope.to_string(), id.to_string());
    h.update(|w, _| w.within(scope).try_find(id).map(|s| s.label().unwrap_or_default().to_string()))
}

pub fn exists_in<C: UiContext>(h: &mut Harness<C>, scope: &str, id: &str) -> bool {
    label_in(h, scope, id).is_some()
}

/// Picks a relationship option inside an embedded form.
pub fn choose_related_in<C: UiContext>(h: &mut Harness<C>, scope: &str, field: &str, option: &str) {
    h.click_in(scope.to_string(), format!("field:{field}"));
    wait(h);
    h.click_in(scope.to_string(), format!("field:{field}:option:{}", crate::ui::widgets::slug(option)));
    wait(h);
}

/// The text of whichever message line (alert or status) the record view shows.
pub fn messages<C: UiContext>(h: &mut Harness<C>) -> String {
    ["status:error", "notice:error", "status:info", "notice:info", "list-error", "list-notice", "rt-notice-error", "rt-notice", "error", "notice"]
        .iter()
        .filter_map(|id| h.try_label(*id))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Waits until some message line contains `text`.
pub fn expect_alert<C: UiContext>(h: &mut Harness<C>, text: &str) {
    eventually(h, &format!("an alert containing {text:?}"), |h| messages(h).contains(text));
}

/// Replaces the document's YAML in Settings > YAML and applies it, as a person would paste it.
pub fn apply_yaml<C: UiContext>(h: &mut Harness<C>, yaml: &str) {
    use crate::ui::shell::{settings::AppSettings, yaml::YamlTab};
    h.switch_mode(crate::ui::shell::ModeId::App);
    h.click("tab-yaml");
    let view = h.read_shell(|s, _| s.mode_view()).expect("settings view");
    let settings: gpui_kit::Entity<AppSettings> = view.downcast().expect("AppSettings");
    let tab = h.cx.read(|cx| settings.read(cx).tab_view()).expect("yaml tab");
    let tab: gpui_kit::Entity<YamlTab> = tab.downcast().expect("YamlTab");
    let editor = h.cx.read(|cx| tab.read(cx).editor.clone());
    let text = yaml.to_string();
    h.update(|window, cx| editor.update(cx, |e, cx| e.set_value(text, window, cx)));
    h.settle();
    h.click("apply-yaml");
    assert_eq!(h.notice().as_deref(), Some("YAML applied."));
}

// ---- reports ---------------------------------------------------------------------------

/// Hosts the Run-mode report page (a `ReportRun`) in a window of its own, as the Runtime would
/// show it, and points the harness at it. Call [`leave_report`] with the returned window to go
/// back. (The Runtime's report page builder is not registered in this build.)
pub fn host_report<C: UiContext>(
    h: &mut Harness<C>,
    report: &str,
    params: Option<Map<String, Value>>,
) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<crate::ui::reports::ReportRun>) {
    let config = h.config();
    let id = config.reports.iter().find(|r| r.name == report).unwrap_or_else(|| panic!("no report {report}")).id.clone();
    let shell = h.shell();
    let studio = h.window;
    let (window, run) = h.cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| crate::ui::reports::ReportRun::new(shell, &id, params, cx))
        })
        .expect("open the report window")
    });
    h.window = window;
    h.settle();
    (studio, run)
}

pub fn leave_report<C: UiContext>(h: &mut Harness<C>, studio: gpui_kit::AnyWindowHandle) {
    h.window = studio;
    h.settle();
}

/// Waits for the first page, then returns the text lines of every page (Next page).
pub fn report_pages<C: UiContext>(h: &mut Harness<C>, run: &gpui_kit::Entity<crate::ui::reports::ReportRun>) -> Vec<Vec<String>> {
    eventually(h, "page 1 of the report", |h| h.try_label("report-page").is_some_and(|l| l.starts_with("Page 1 of")));
    let label = h.label("report-page");
    let pages: usize = label.rsplit(" of ").next().and_then(|n| n.parse().ok()).expect("page count");
    let mut out = vec![];
    for page in 1..=pages {
        let want = format!("Page {page} of {pages}");
        eventually(h, &want, |h| h.try_label("report-page").as_deref() == Some(want.as_str()));
        out.push(report_page_lines(h, run));
        if page < pages {
            h.click("next-page");
        }
    }
    out
}

/// The text lines of the page the preview shows.
pub fn report_page_lines<C: UiContext>(h: &mut Harness<C>, run: &gpui_kit::Entity<crate::ui::reports::ReportRun>) -> Vec<String> {
    let preview = h.cx.read(|cx| run.read(cx).preview.clone()).expect("a preview");
    h.cx.read(|cx| preview.read(cx).page_texts())
}

// ---- dashboards ------------------------------------------------------------------------

/// What a dashboard component shows, computed through the backend: the Dashboards UI is not
/// part of this port, so KPI values are the saved query's rows evaluated with the
/// component's expression and format (`rows` and `params` in scope).
pub struct DashboardView {
    label: String,
    dashboard: crate::dashboards::Dashboard,
    pub filters: Map<String, Value>,
}

pub fn dashboard(h: &Harness, name: &str) -> DashboardView {
    let config = h.config();
    let dashboard = config.dashboards.iter().find(|d| d.name == name).unwrap_or_else(|| panic!("no dashboard {name}")).clone();
    DashboardView { label: h.label.clone(), dashboard, filters: Map::new() }
}

impl DashboardView {
    /// Chooses a filter by its name ("Stage") the way the filter control would.
    pub fn filter(&mut self, name: &str, value: &str) {
        let f = self.dashboard.filters.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no filter {name}"));
        self.filters.insert(f.param.clone(), json!(value));
    }

    /// The saved query's rows as objects; the filter values the query declares are passed.
    pub fn rows(&self, query: &str) -> Vec<Value> {
        let config = crate::read_document_config(self.label.clone()).expect("config");
        let q = config.saved_queries.iter().find(|q| q.id == query).unwrap_or_else(|| panic!("no query {query}"));
        let params: Vec<NamedValue> = q
            .parameters
            .iter()
            .filter_map(|p| self.filters.get(&p.name).map(|v| NamedValue { column: p.name.clone(), value: to_data_value(v) }))
            .collect();
        let run = tauri::async_runtime::block_on(crate::queries::run_saved_query(
            self.label.clone(), q.id.clone(), params, None, None,
        ))
        .expect("dashboard query");
        run.rows
            .iter()
            .map(|row| Value::Object(run.columns.iter().cloned().zip(row.iter().map(plain)).collect()))
            .collect()
    }

    fn component(&self, title: &str) -> &crate::dashboards::DashboardComponent {
        self.dashboard.components.iter().find(|c| c.title == title).unwrap_or_else(|| panic!("no component {title}"))
    }

    /// A KPI's shown value, formatted like the card.
    pub fn kpi(&self, title: &str) -> String {
        let c = self.component(title);
        let rows = self.rows(c.query_id.as_deref().expect("a query"));
        let mut scope = crate::expr::Scope::new();
        scope.insert("rows".into(), Value::Array(rows.clone()));
        scope.insert("params".into(), Value::Object(self.filters.clone()));
        let value = match &c.expression {
            Some(source) => crate::expr::evaluate(source, &scope, Default::default()).expect("kpi expression"),
            None => rows.first().and_then(|r| r.get(c.value_field.as_deref().unwrap_or_default())).cloned().unwrap_or(Value::Null),
        };
        crate::expr::format_value(&value, c.format.as_deref()).expect("format")
    }

    /// A table component's rows, projected to its columns.
    pub fn table(&self, title: &str) -> Vec<Value> {
        let c = self.component(title);
        self.rows(c.query_id.as_deref().expect("a query"))
    }

    /// The choices of a select filter, from its options query's first column.
    pub fn options(&self, filter: &str) -> Vec<String> {
        let f = self.dashboard.filters.iter().find(|f| f.name == filter).unwrap_or_else(|| panic!("no filter {filter}"));
        let id = f.options_query_id.clone().expect("an options query");
        self.rows(&id)
            .iter()
            .filter_map(|r| r.as_object().and_then(|o| o.values().next()).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())))
            .collect()
    }
}

// ---- direct record writes --------------------------------------------------------------

pub fn named(column: &str, value: DataValue) -> NamedValue {
    NamedValue { column: column.into(), value }
}

/// Inserts a row through the backend's public write command (no UI).
pub fn insert_direct(
    label: &str,
    table: &str,
    values: Vec<NamedValue>,
) -> Result<crate::recordstore::commands::TriggeredOutcome, crate::manager::AppError> {
    crate::recordstore::commands::insert_row(label.into(), table.into(), values, None)
}

/// Updates a row through the backend's public write command (no UI).
pub fn update_direct(
    label: &str,
    table: &str,
    values: Vec<NamedValue>,
    identity: Vec<DataValue>,
    expected: Vec<NamedValue>,
) -> Result<crate::recordstore::commands::TriggeredOutcome, crate::manager::AppError> {
    crate::recordstore::commands::update_row(label.into(), table.into(), values, identity, Some(expected), None)
}
