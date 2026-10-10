# Access import fixtures

- `template/`: the parts of a small Access template package (`.accdt`).
  Tests zip the folder into a package at run time. It covers tables with
  lookups, an attachment and a multi-value field, a calculated column, a
  relationship, select, union and append queries, list and detail forms
  (tab control, subform, embedded macros in AXL and in legacy rows, a
  UTF-16 part with VBA code-behind), a grouped report, an AutoExec macro
  and a VBA module.
- `orders.accdb.gz` (ACE, Access 2010 format) and `orders.mdb.gz` (Jet 4):
  two tables, a relationship, field properties (lookups, validation rules,
  defaults, formats), and four saved queries written as `MSysQueries` rows.
  Built by `scripts/access/MakeFixture.java`.
- `complex-data.accdb.gz`: attachment, multi-value and version-history
  columns. It is `complexDataV2010.accdb` from the Jackcess test data
  (https://github.com/spannm/jackcess), Apache License 2.0.

The binary fixtures are gzipped because Access pages are mostly empty.
