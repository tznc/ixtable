//! Data mode: tables, schema designer, relationships and the record grid
//! (port of `src/data/`).

pub mod create_table;
pub mod designer;
pub mod diagram;
pub mod layout;
pub mod relate;
pub mod relationships;
pub mod sheet;
pub mod workbench;

#[cfg(test)]
mod tests_logic;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub mod testkit;
#[cfg(test)]
mod tests_sheet;
#[cfg(test)]
mod tests_schema;
