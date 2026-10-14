---
sidebar_position: 1
---

# Queries

_A saved query is a named, read-only SQL statement with typed parameters. Forms, reports, dashboards, and actions read data through saved queries._

You build a query visually or write it in SQL, then save it in the project. ixtable runs every query through DuckDB against the application's records, whether they live in the embedded SQLite database or in PostgreSQL. Queries only read. Records change through forms and [actions](./automation), and the schema changes through the table designer and [migrations](./migrations).

Queries live on the Data page. Choose a query under **Queries** in the object browser to open it, or choose **New query** (the + beside **Queries**). To start from a table, open the table and choose **Query this table**.

## Create and save a query

Choose **New query**. The new query opens in the visual builder with the name "Untitled query", and the object browser lists it as unsaved. Give it a name and choose **Save query**. Names must be unique, ignoring case.

To rename a saved query, change its name and choose **Save query**. **Duplicate** and **Delete** sit beside **Save query**. You can look at a table and come back to a query without losing unsaved changes. Opening another query or starting a new one asks whether to discard them.

When you save a query written in SQL, ixtable checks it against the database. A query that names an unknown table or column does not save.

## The visual builder

The **Builder** tab builds a query in panels, from top to bottom.

| Panel | What it sets |
| --- | --- |
| Sources | The table or view to read from, and joins to other tables |
| Fields | The columns to return, with an optional aggregate and output name for each |
| Filters | Conditions on rows, combined with all or any, in nested groups |
| Group by | Columns to group by |
| Having | Conditions on aggregated values |
| Sort and limit | Sort order and a maximum row count |

**Sources** suggests joins from the table's foreign keys, such as `Join customers on o.customer_id = customers.id`. You can also add a join by hand: inner, left, right, or full outer, with one or more equality conditions.

**Fields** returns every column when you choose none. Aggregates are count, count distinct, sum, average, min, and max. When any field has an aggregate, the builder groups by the other fields. **Add row count** adds `count(*)`.

Filter operators are equals, does not equal, less than, at most, greater than, at least, contains, starts with, is empty, and is not empty. Contains and starts with ignore case. Is empty tests for null. A filter compares with a fixed value or with a parameter.

The builder shows a live preview of the first 100 rows as you work.

## SQL

The **SQL** tab shows the SQL the builder generates. **Switch to SQL** turns the query into an SQL query you edit by hand. The switch is one way, because the builder cannot read arbitrary SQL. On an SQL query, **Start visual builder** replaces the SQL with a new builder query.

Write SQL in the DuckDB dialect. Use SQL for what the builder lacks, such as computed columns, `DISTINCT`, `IN` lists, subqueries, common table expressions, `UNION`, and joins on conditions other than equality.

```sql
SELECT c.name, sum(o.total) AS revenue
FROM orders o
JOIN customers c ON c.id = o.customer_id
WHERE o.placed_at >= $since
GROUP BY c.name
ORDER BY revenue DESC
```

A saved query is one read-only statement that starts with `SELECT`, `WITH`, `VALUES`, `SHOW`, or `DESCRIBE`. ixtable refuses statements that write, change the schema, attach databases, change settings, or read files. A trailing semicolon is fine.

## Parameters

A parameter is a named value the query receives when it runs. Write it as `$name` in SQL, or pick it as a filter value in the builder. Positional parameters such as `?` and `$1` are not supported. ixtable binds every value as a typed value and never pastes it into the SQL text.

The **Parameters** panel declares each parameter with a type, an optional default, and a **Required** flag. When the SQL uses a name that is not declared yet, the panel offers **Declare $name**.

| Type | Accepts |
| --- | --- |
| text | Any text |
| integer | Whole numbers |
| number | Decimal numbers |
| boolean | `true`, `yes`, `1`, `false`, `no`, `0` |
| date | `YYYY-MM-DD` |
| timestamp | An ISO date and time, or a plain date |

When a query runs, a supplied value wins, then the default, then null. A required parameter with no value and no default fails. A saved query that forms, reports, dashboards, or actions run must declare every parameter it uses.

## Run a query

The **Run with** fields supply parameter values for a test run. Leave a field blank to use the default. Choose **Run** to see the results with the row count and the time taken.

A run returns at most 10,000 rows and says so when it stops there. When a query takes longer than two seconds, a progress bar and **Cancel query** appear. Starting another run or leaving the editor cancels the previous run.

## Where saved queries are used

| Where | How |
| --- | --- |
| [Forms](./runtime-forms) | A form's source can be a saved query. The form is read-only and supports list and detail modes. Its parameters are expressions over `app` and `params` |
| Select controls | **Options from saved query** fills a list from the first column, with an optional label in the second |
| [Reports](./reports) | A report's dataset, and the rows of a table component |
| [Dashboards](./dashboards) | KPIs, charts, tables, and the choices of a drop-down filter. Filters set query parameters by name |
| [Actions](./automation) | The Run query step runs a query with parameter values and stores its rows |

## Permissions

Each saved query is an object in the Roles tab. A role can run a query when it has read on the query, or read on a form, report, or dashboard that shows the query. An action step still needs read on its query. Filters do not limit which rows a role can read, so grant a narrower query when a role must see less. Ad hoc SQL in the query editor is available only to the developer, never to a role. See [Roles and permissions](./roles).

## Next steps

- [Data view](../concepts/data-view)
- [Reports](./reports)
- [Dashboards](./dashboards)
- [Expressions](../reference/expressions)
