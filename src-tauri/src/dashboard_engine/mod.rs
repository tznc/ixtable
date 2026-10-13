//! Non-UI dashboard logic (port of `src/dashboards/{model,data,useDashboardData,
//! EmbeddedForm}.ts*` and `src/dashboards/charts/`): definition helpers, query results to
//! KPI values and chart data, filter parameters, chart figures (drawn with the report
//! engine's chart geometry) and the first-row binding of embedded forms. The GPUI views are
//! in `ui::dashboards`.

pub mod binding;
pub mod chart;
pub mod data;
pub mod load;
pub mod model;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_chart;
