//! Golden-app suites (PRD §26.5): the CRM, Inventory and Work orders templates driven through
//! the real UI and backend, with a JSON evidence recorder. Port of
//! `tests/integration/golden/*.test.tsx` and `tests/integration/journey-evidence.test.tsx`.
//! CI runs them with the filter `ui::golden`.
//!
//! Steps that need the Design studio or the Dashboards UI go through the ConfigStore and
//! the backend instead; each one says so where it happens.

pub mod journey;

#[cfg(test)]
mod tests_crm;
#[cfg(test)]
mod tests_definitions;
#[cfg(test)]
mod tests_evidence;
#[cfg(test)]
mod tests_inventory;
#[cfg(test)]
mod tests_reports;
#[cfg(test)]
mod tests_work_orders;
