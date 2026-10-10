//! Runtime authorization at command entry points (PRD §20.2, §27.2).
//!
//! Each session holds its runtime role (`Session::access`). It is set only
//! from trusted sources: the re-verified signed manifest when a cloud
//! installation opens (`cloud::install`), or the Studio "Preview as role"
//! switch (`set_runtime_role_preview`). Developer sessions without a role are
//! unrestricted; a cloud installation whose role was never set is allowed
//! nothing (fail closed).
//!
//! The checks mirror `can()` in src/runtime/rbac.ts, plus the access the
//! runtime UI needs to show what a role may open: a form grants its source
//! table, lookup and related-list tables, and option queries; a report
//! grants its dataset table and queries; a dashboard grants read on the
//! queries of its KPI, table and chart components and filter options (never
//! embedded forms or reports, their queries or tables, or any write or
//! action). Dashboard filters and columns are not row or column security:
//! the caller supplies parameters, filters and limits (PRD §20.1). Ad hoc SQL
//! is refused for a role. This is not a defense against a user holding direct database credentials.
use crate::archive::DocumentConfig;
use crate::dashboards::{ComponentKind, Dashboard};
use crate::design::{Form, SourceKind};
use crate::manager::{AppError, Session};
use crate::reports::{Band, Report};
use crate::roles::Role;
use std::collections::HashSet;
use std::sync::Mutex;

/// The readable-table set of one role, cached per session. Computing it runs
/// one DuckDB schema query per granted base table, so it is kept until the
/// role, the config or the data (schema) changes (`clear`).
#[derive(Debug, Default)]
pub struct ReadableCache(Mutex<Option<(Role, HashSet<String>)>>);

impl ReadableCache {
    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    /// The cached set for `role`, computing it with `compute` on a miss.
    pub fn get_or(
        &self,
        role: &Role,
        compute: impl FnOnce() -> HashSet<String>,
    ) -> HashSet<String> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref() {
            Some((cached, set)) if cached == role => set.clone(),
            _ => {
                let set = compute();
                *slot = Some((role.clone(), set.clone()));
                set
            }
        }
    }
}

/// Role id of a cloud user whose manifest assigns no role.
pub const NO_ROLE: &str = "cloud:no-role";

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Access {
    /// No role set: developer access, except in a cloud installation.
    #[default]
    Unset,
    /// Explicit developer access (the signed cloud owner).
    Unrestricted,
    Role(Role),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read,
    Create,
    Update,
    Delete,
    Execute,
}

impl Op {
    fn label(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Create => "create",
            Op::Update => "change",
            Op::Delete => "delete",
            Op::Execute => "run",
        }
    }
}

/// A role allowed nothing.
pub fn deny_all() -> Role {
    Role {
        id: NO_ROLE.into(),
        name: "Runtime user".into(),
        permissions: Default::default(),
    }
}

fn is_cloud_session(s: &Session) -> bool {
    crate::installation::runtime_session(&s.id)
        .is_some_and(|rt| rt.dir.starts_with(crate::cloud::install::cloud_root()))
}

/// The role enforced for a session; None means unrestricted.
pub fn effective_role(s: &Session) -> Option<Role> {
    match &s.access {
        Access::Role(role) => Some(role.clone()),
        Access::Unrestricted => None,
        Access::Unset if is_cloud_session(s) => Some(deny_all()),
        Access::Unset => None,
    }
}

/// Sets the window's role from a trusted source (None: developer access).
pub fn set_role(window: &str, role: Option<Role>) -> Result<(), AppError> {
    crate::manager()?.with_session(window, |s| {
        s.access = role.map_or(Access::Unrestricted, Access::Role);
        Ok(())
    })
}

/// The permission entry flag the role grants explicitly (rbac.ts `can`).
fn explicit(role: &Role, kind: &str, id: &str, op: Op) -> bool {
    let p = &role.permissions;
    match kind {
        "navigation" => p.navigation.iter().any(|n| n == id),
        "action" => op == Op::Execute && p.actions.iter().any(|a| a == id),
        _ => p
            .objects
            .iter()
            .find(|o| o.kind == kind && o.id == id)
            .is_some_and(|o| match op {
                Op::Read => o.read,
                Op::Create => o.create,
                Op::Update => o.update,
                Op::Delete => o.delete,
                Op::Execute => false,
            }),
    }
}

/// Any flag on the object (rbac.ts sets `read` whenever another flag is set).
fn granted(role: &Role, kind: &str, id: &str) -> bool {
    [Op::Read, Op::Create, Op::Update, Op::Delete]
        .into_iter()
        .any(|op| explicit(role, kind, id, op))
}

fn source_table(form: &Form) -> Option<&str> {
    form.source
        .as_ref()
        .filter(|s| s.kind == SourceKind::Table)
        .and_then(|s| s.table.as_deref())
}

pub(crate) fn bands(report: &Report) -> Vec<&Band> {
    let b = &report.bands;
    let mut out = vec![&b.report_header, &b.page_header, &b.detail];
    for g in &b.groups {
        out.extend([&g.header, &g.footer]);
    }
    out.extend([&b.page_footer, &b.report_footer]);
    out
}

/// Tables the role may read: granted tables, form source tables, report
/// tables, form lookup and related-list tables, and foreign-key targets of
/// those base tables (`fk_targets`, the list lookups the runtime shows).
fn readable_tables(
    config: &DocumentConfig,
    role: &Role,
    fk_targets: &dyn Fn(&str) -> Vec<String>,
) -> HashSet<String> {
    let mut base: HashSet<String> = role
        .permissions
        .objects
        .iter()
        .filter(|o| o.kind == "table" && granted(role, "table", &o.id))
        .map(|o| o.id.clone())
        .collect();
    let mut extra = HashSet::new();
    for form in &config.design.forms {
        if !granted(role, "form", &form.id) {
            continue;
        }
        base.extend(source_table(form).map(str::to_string));
        for c in &form.controls {
            extra.extend(c.relationship.as_ref().map(|r| r.table.clone()));
            if let Some(related) = &c.related {
                extra.insert(related.table.clone());
            }
        }
    }
    for report in &config.reports {
        if explicit(role, "report", &report.id, Op::Read) {
            base.extend(report.table.clone());
        }
    }
    for table in &base {
        extra.extend(fk_targets(table));
    }
    base.extend(extra);
    base
}

/// Saved queries a form reads: a query source and option queries.
pub(crate) fn form_queries(form: &Form) -> impl Iterator<Item = &str> {
    let source = form.source.as_ref().and_then(|s| s.query_id.as_deref());
    let options = form
        .controls
        .iter()
        .filter_map(|c| c.options_query_id.as_deref());
    source.into_iter().chain(options)
}

/// Saved queries a dashboard reads: KPI, table and chart queries, and filter
/// choices. Embedded forms are excluded: their queries need the form grant
/// (`readable_query`'s `by_form`), since the dashboard refuses forms the role
/// cannot open.
pub(crate) fn dashboard_queries(dashboard: &Dashboard) -> impl Iterator<Item = &str> {
    let components = dashboard
        .components
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ComponentKind::Kpi | ComponentKind::Table | ComponentKind::Chart
            )
        })
        .filter_map(|c| c.query_id.as_deref());
    let filters = dashboard
        .filters
        .iter()
        .filter_map(|f| f.options_query_id.as_deref());
    components.chain(filters)
}

fn readable_query(config: &DocumentConfig, role: &Role, id: &str) -> bool {
    let by_form = config
        .design
        .forms
        .iter()
        .filter(|f| granted(role, "form", &f.id))
        .any(|f| form_queries(f).any(|q| q == id));
    let by_report = || {
        config
            .reports
            .iter()
            .filter(|r| explicit(role, "report", &r.id, Op::Read))
            .any(|r| {
                r.dataset_query_id.as_deref() == Some(id)
                    || bands(r).iter().any(|b| {
                        b.components
                            .iter()
                            .any(|c| c.extra.get("queryId").and_then(|v| v.as_str()) == Some(id))
                    })
            })
    };
    let by_dashboard = || {
        config
            .dashboards
            .iter()
            .filter(|d| explicit(role, "dashboard", &d.id, Op::Read))
            .any(|d| dashboard_queries(d).any(|q| q == id))
    };
    by_form || by_report() || by_dashboard()
}

/// Whether `role` may perform `op` on the object (see module docs).
pub fn allows(
    config: &DocumentConfig,
    role: &Role,
    kind: &str,
    id: &str,
    op: Op,
    fk_targets: &dyn Fn(&str) -> Vec<String>,
) -> bool {
    allows_with(config, role, kind, id, op, &|| {
        readable_tables(config, role, fk_targets)
    })
}

/// `allows`, with the readable-table set supplied (possibly cached).
fn allows_with(
    config: &DocumentConfig,
    role: &Role,
    kind: &str,
    id: &str,
    op: Op,
    readable: &dyn Fn() -> HashSet<String>,
) -> bool {
    if explicit(role, kind, id, op) {
        return true;
    }
    match (kind, op) {
        ("table", _) => {
            let by_form = config
                .design
                .forms
                .iter()
                .any(|f| source_table(f) == Some(id) && explicit(role, "form", &f.id, op));
            by_form || (op == Op::Read && readable().contains(id))
        }
        ("query", Op::Read) => readable_query(config, role, id),
        _ => false,
    }
}

fn forbidden(role: &Role, message: String) -> AppError {
    AppError::new("FORBIDDEN", format!("The role \"{}\" {message}", role.name))
}

/// Checks one object operation for the window's session.
pub fn check(window: &str, kind: &str, id: &str, op: Op) -> Result<(), AppError> {
    crate::manager()?.with_session(window, |s| check_session(s, kind, id, op))
}

pub fn check_session(s: &Session, kind: &str, id: &str, op: Op) -> Result<(), AppError> {
    let Some(role) = effective_role(s) else {
        return Ok(());
    };
    if allows_with(&s.doc.config, &role, kind, id, op, &|| {
        readable_set(s, &role)
    }) {
        Ok(())
    } else {
        Err(forbidden(
            &role,
            format!("cannot {} {kind} \"{id}\"", op.label()),
        ))
    }
}

/// The tables `role` may read in this session (cached, see `ReadableCache`).
fn readable_set(s: &Session, role: &Role) -> HashSet<String> {
    s.authz_cache.get_or(role, || {
        let fk = |table: &str| {
            s.reader
                .schema(table)
                .map(|t| t.foreign_keys.into_iter().map(|k| k.target_table).collect())
                .unwrap_or_default()
        };
        readable_tables(&s.doc.config, role, &fk)
    })
}

/// Whether the session may read `table` (None: unrestricted).
pub fn can_read_table(s: &Session, table: &str) -> bool {
    check_session(s, "table", table, Op::Read).is_ok()
}

/// Refuses operations no runtime role may perform (ad hoc SQL, attachment export).
pub fn require_unrestricted(window: &str, what: &str) -> Result<(), AppError> {
    crate::manager()?.with_session(window, |s| unrestricted_session(s, what))
}

pub fn unrestricted_session(s: &Session, what: &str) -> Result<(), AppError> {
    match effective_role(s) {
        Some(role) => Err(forbidden(&role, format!("cannot {what}"))),
        None => Ok(()),
    }
}

/// "Preview as role" in Studio: Run mode enforces the previewed role in Rust
/// exactly as an installed runtime would. `role_id` None ends the preview.
/// A cloud installation's role comes from its signed manifest and cannot change.
#[tauri::command]
pub fn set_runtime_role_preview(
    window_label: String,
    role_id: Option<String>,
) -> Result<(), AppError> {
    crate::manager()?.with_session(&window_label, |s| preview_role(s, role_id))
}

pub fn preview_role(s: &mut Session, role_id: Option<String>) -> Result<(), AppError> {
    if is_cloud_session(s) {
        return Err(AppError::new(
            "FORBIDDEN",
            "This application's role is assigned by ixtable Cloud",
        ));
    }
    s.access = match role_id {
        None => Access::Unset,
        Some(id) => Access::Role(
            s.doc
                .config
                .roles
                .iter()
                .find(|r| r.id == id)
                .cloned()
                // Unknown roles are denied everything (rbac.ts).
                .unwrap_or(Role { id, ..deny_all() }),
        ),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("authz_tests.rs");
}
