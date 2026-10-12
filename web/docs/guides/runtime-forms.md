---
sidebar_position: 2
---

# Runtime forms

_The Runtime shows the application the way its users see it: a navigation menu, list pages, and record forms for viewing, creating, and editing records._

You build forms in [Design view](../concepts/design-view). The Runtime renders the same form definitions, with validation, computed fields, conditions, related records, and buttons that run actions. Every save goes straight to the application's database, and every list reads the current records.

## Open the Runtime

In Studio, choose **Runtime** in the mode switch. Studio shows the application with its navigation, and a **Preview as role** menu lets you check what each [role](./roles) can see. Your users open the application from a [runtime bundle](./runtime-bundles), which shows only the Runtime, with no design modes.

An application with no navigation items shows "This application has no pages yet." In Studio, **Generate app from tables** builds a list form and a detail form for every table. The detail forms get lookups for foreign keys and related lists for child tables.

## Navigation

The navigation menu on the left lists forms, reports, dashboards, and tables, grouped the way you arranged them in the navigation editor in Design view. A role sees an item only when the role lists it and can read what it opens. Empty groups disappear.

The application opens on the start page set in the navigation editor, or on the first item. A page opened by a button or an action stacks on the current page, and a back link above it returns to the previous one. Choosing an item in the menu starts a new trail.

A table item opens a generated list and detail form for that table. These generated forms are not saved in the project.

## Form modes

A form has up to four modes: list, detail, create, and edit. You choose which modes a form supports in Design view, and a navigation item can open a form in a specific mode. A form whose source is a saved query is read-only, so it has no create or edit mode.

| Mode | What the user sees | Buttons |
| --- | --- | --- |
| List | A page of rows | **New** opens create mode. Selecting a row opens it in detail mode |
| Detail | One record, read-only | **Edit** and **Delete**, when the role allows them |
| Create | An empty record | **Create** and **Cancel** |
| Edit | The record with editable fields | **Save** and **Cancel** |

A list form opens rows in itself or in the detail form set in its **Row opens** property. Cancel in edit mode returns to detail mode. Cancel in create mode returns to the previous page.

## List pages

A list page shows the columns chosen in the form's list settings, or its bound fields. Lookup columns show the related record's display value instead of its key.

- **Search** matches text in one column at a time. Pick the column, then type.
- **Sort** by selecting a column header. Each selection cycles ascending, descending, and off.
- **Paging** uses the form's rows-per-page setting, 25 by default. The pager shows the range and the total.

A list form can also have a **Row filter**, an [expression](../reference/expressions) such as `record.status = 'open'`. On a table, ixtable hands the simple parts of the filter to the database: number comparisons on integer and real columns, equality tests on text columns, `in` lists of those, and `is null` tests, joined with `and`. A filter made only of these parts reads one page at a time with an exact total.

Anything else, such as `or`, arithmetic on the row, text ordering, or comparisons of dates and decimals, runs in the app on the rows the database returns. That part scans at most 50,000 rows. When it stops there, the list says that some matches may be missing. Filters on query-sourced forms always run in the app.

## Create, edit, and delete

Create mode fills each field from its default value expression. Saving writes the record, shows "Record created.", and switches to detail mode. Edit mode locks primary key fields and writes only the fields that changed. Pressing Enter in a text field saves.

**Delete** asks for confirmation, then removes the record. Leaving a form in create or edit mode discards unsaved changes without asking.

## Validation

Each field can have rules: **Required**, **Minimum**, **Maximum**, **Pattern**, and a **Validation rule** expression. A form can also have form-level rules that compare fields, such as `record.end >= record.start`. Each rule has a default message, and a custom **Error message** replaces it.

ixtable checks a field when the user leaves it and checks the whole form on save. Errors appear under each field. A tab that contains an error says so in its label. Hidden, disabled, and read-only fields are not validated. Required fields show an asterisk.

## Computed fields and conditions

A computed field shows the result of an expression, such as `record.quantity * record.unitPrice`, and updates as the user types. An input field with a **Computed value** becomes read-only, and ixtable writes the computed value to its column on save.

**Visible when** and **Enabled when** take expressions that decide whether a field, section, or tab shows and accepts input. A condition that fails counts as false. A disabled field keeps its stored value on save. These conditions change what a person sees. They are not access control.

Expressions on a form read the record as `record`, the form state as `form`, and the application state as `app`. `app.user` and `app.role` describe who is running the application.

## Related records

A **lookup** field picks a related record. It offers a search box and up to 50 matching choices, and an optional choice filter narrows them. A lookup on a multi-column key writes every key column.

A **related list** shows child records inside a parent form, such as the orders of a customer. It appears once the parent is saved. **Add** opens a create form for a child with the link to the parent filled in and locked. Each row has **Edit** and **Delete** buttons. Editing a row opens its child form inside the list, and that form shows its own related lists, such as the lines of an order. Related lists nest up to three levels below the form you open, for example customers, orders, order lines, and stock allocations. Studio offers only child forms that stay within three levels and never lead back to the form itself.

## Buttons

A button runs an [action](./automation). The button is disabled while the record loads, while another action runs, and when the role cannot run the action. The action can update records, open other pages, ask for confirmation, and show messages. An error shows next to the form.

Saving a record also runs any [triggers](./automation#triggers) on its table. When a synchronous trigger fails, the record stays saved and the form shows the trigger's error.

## Conflicting edits

Each table has a concurrency policy, set in Settings, then **Entities**. New tables use the optimistic policy.

| Policy | Behavior |
| --- | --- |
| Optimistic: reject stale edits | A save compares the values the user started from with the stored row. If someone else changed the row first, the save fails |
| Last write wins | The latest save replaces the row |
| Custom transactional action | Updates and deletes run an action you choose instead of writing the row |

When an optimistic save fails, the form says that someone else changed the record. The user goes back, opens the record again, and repeats the edit.

## Keyboard

Rows in a list open with Enter or Space. Tabs move with the arrow keys. The record heading takes focus when a record loads. The delete confirmation traps focus, and Escape cancels it.

## Next steps

- [Design view](../concepts/design-view)
- [Expressions](../reference/expressions)
- [Automation](./automation)
- [Roles and permissions](./roles)
