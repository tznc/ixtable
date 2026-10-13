//! Schema design: logical types, column drafts and the shared editors for columns,
//! constraints and store capabilities (port of `src/schema/`).

pub mod columns;
pub mod logical;

#[cfg(test)]
pub mod testkit;
#[cfg(test)]
mod tests;
