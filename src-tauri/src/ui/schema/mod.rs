//! Schema design: logical types, column drafts and the shared editors for columns,
//! constraints and store capabilities (port of `src/schema/`).

pub mod capability;
pub mod column_fields;
pub mod columns;
pub mod constraints;
pub mod controls;
pub mod datasource;
pub mod entities;
pub mod impact;
pub mod logical;

#[cfg(test)]
pub mod testkit;
#[cfg(test)]
mod tests;
