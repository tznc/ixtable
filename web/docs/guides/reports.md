---
sidebar_position: 3
---

# Reports

_Reports lay out records on printable pages with bands, groups, and totals. You preview them page by page, print them, or save them as PDF._

A report reads rows from a saved query or a table. It places components on bands, such as a header, a detail band for each row, and footers for groups and the whole report. ixtable paginates the result the same way on every computer, so the preview, the printout, and the PDF match. Open Reports mode in Studio and choose **New report**.

## Dataset

The **Dataset** picker in the toolbar chooses where rows come from: a saved [query](./queries), a table, or no dataset. A report without a dataset prints its bands once, which suits a cover page or a form letter.

When the saved query declares parameters, a **Parameters** section shows one field per parameter. These are the report's default values. An action's Open report step and a dashboard's filters can pass other values when they open the report. Report expressions read the values as `params`. In the Runtime, a report that is missing a value asks for it first, as [Reports in the Runtime](#reports-in-the-runtime) describes.

A report reads at most 100,000 rows from a query. Beyond that, the preview says that it shows only the first 100,000.

## Bands

Bands print in this order:

| Band | Prints |
| --- | --- |
| Report header | Once, at the start |
| Page header | At the top of every page |
| Group header | Before each group, outermost group first |
| Detail | Once per row |
| Group footer | After each group, innermost group first |
| Page footer | At the bottom of every page |
| Report footer | Once, at the end |

Select a band by its label on the canvas. The band's properties set its **Band height**, **Keep together**, **Page break before**, and **Page break after**. Drag a band's lower edge to change its height. A band with no height and no components does not print.

## Components

The component toolbar adds components to the selected band.

| Component | Shows |
| --- | --- |
| Text | Fixed text |
| Field | An [expression](../reference/expressions) over the current row, such as `record.amount` |
| Calculated | An expression over the band's rows, such as `sum(rows.amount)` |
| Image | An image from the project's assets |
| Line | A horizontal or vertical line |
| Rectangle | A box with a border and optional fill |
| Table | Rows as a table with headers, from the band's rows or from another saved query |
| Subreport | Another report printed inside the band, such as the lines of an invoice |

Drag a component to move it and drag its corner to resize it. With a component focused, the arrow keys move it by 1 point, or 10 with Shift. Alt and the arrow keys resize it, and Delete removes it. **Position and size** sets exact values in points.

Field and calculated components take an optional **Format**, such as `#,##0.00` or `MMM d, yyyy`. Text components can set the font size, alignment, and bold. Shapes can set the border width and a gray fill. A band holds at most one table, and page headers and footers cannot hold tables or subreports.

**Can grow** on a text, field, or calculated component lets its box grow to fit its text. Without it, lines beyond the box's height are cut off. A growing box pushes the components below it down, and the band grows with it. Components beside the box stay where they are.

Can grow has no effect in page headers and footers, which keep their height, or in a band that holds a table. In a band with a table, the preview lists a problem. Page numbers do not make a box grow, because the box is measured before pages are counted.

## Subreports

A **subreport** prints another report inside a band, once for each instance of the band. Use it for invoices and statements: an invoice report prints each order, and a subreport in its detail band prints that order's lines.

- **Report** picks the report to print. Studio offers only reports that keep the nesting within three levels and never print the report that contains them.
- **Links** match the subreport's rows to the band's row. Each link names a child field in the subreport's rows and a parent field in the band's row, such as `order_id` and `id`. A row prints when every child field equals its parent field. Without links, every row prints.

The subreport prints its report header, groups, detail band, and report footer at the subreport's left edge. Its page header and page footer don't print, and its page setup is ignored. The subreport takes the height its rows need, and components below it in the band move up or down to follow it. When no rows match, it prints nothing.

Expressions in a subreport read its own `record`, `rows`, and `group`, and `parent` is the row of the band that holds it. A long subreport splits across pages between its bands. A band holds at most one subreport, can't hold both a subreport and a table, and page headers and footers can't hold subreports. Subreports nest up to three levels deep.

## Expressions in reports

Report expressions read these names:

| Name | Value |
| --- | --- |
| `record` | The current row in the detail band. The first row in the report header and the last row in the report footer |
| `rows` | The rows of the current group, or of the whole report |
| `group` | The current group's `key`, `level`, and `count` |
| `rowNumber` | The detail row's number, from 1 |
| `params` | The report's parameter values |
| `parent` | In a subreport, the row of the band that holds it |
| `page`, `pages` | The page number and the total pages |
| `groupPage`, `groupPages` | The page number within the group and the group's total pages |

There is no separate totals component. Put a calculated component in a group footer or the report footer, such as `sum(rows.amount)` or `countof(rows, item.paid)`. For page numbers, use `'Page ' & page & ' of ' & pages`.

An expression that fails prints `#Error`, and the preview lists the problem.

## Groups

**Add group** adds a level of grouping. Each group has a field or an expression to group by, and these options:

- **Descending** reverses the group order.
- **Starts a new page** begins each group on a new page.
- **Repeats header on each page** prints the group header again when a group continues onto another page.
- **Restarts group page numbers** restarts `groupPage` and `groupPages` for each group. It also starts each group on a new page.

ixtable sorts rows by the group keys. Without groups, rows keep the order of the dataset, so sort them in the saved query.

## Page setup and pagination

**Page setup** sets the page size, A4 or Letter, the orientation, and the four margins in points.

- A band that does not fit moves to the next page. Only bands with a table or with grown text split.
- A table grows to fit its rows and pushes the components below it down. A long table splits between rows and repeats its header row on each page.
- A band with grown text splits between lines. The text continues at the top of the next page, below the page header, and the components under it follow. An image or line that would cross the page edge moves to the next page with the text below it.
- **Keep together** moves a band with a table or grown text to the next page whole, as long as it fits on one page. On a group header, it keeps the header on the same page as the group's first row.
- Page breaks never create an empty page.

## Preview, print, and PDF

The **Preview** tab shows one page at a time, with page buttons and a zoom setting. It reloads when the report, its parameters, or its query change. When loading takes more than two seconds, it shows progress and a **Cancel** button.

**Print** opens the system print dialog. **Export PDF…** asks where to save the file and writes a PDF of every page. The same report always exports the same bytes, on every computer.

The PDF prints Western European text in Helvetica. Other characters use two fonts that ship with ixtable, so the output never depends on the fonts installed on a computer. DejaVu Sans covers Greek, Cyrillic, Armenian, Georgian, extended Latin, and symbols. Droid Sans Fallback covers Chinese and Japanese characters.

The PDF embeds only the characters the report uses. Bold text in these fonts prints with thickened outlines, because no bold version ships.

Some text still prints as `?`:

| Text | Why |
| --- | --- |
| Hebrew, Arabic, Syriac, Thaana, and N'Ko | Right-to-left scripts need shaping and reordering that reports do not do |
| Korean Hangul syllables, Thai, and Devanagari and other Indic scripts | Neither bundled font has them |
| Any other character that neither font has | No glyph to print |

Lao text and combining accents print, but without shaping, so their marks can sit in the wrong place. The preview shows the same substitutions as the PDF.

The PDF includes `.jpg` and `.png` images, including PNG images with transparency, interlacing, or 16-bit color. Other image formats print as a placeholder box. So does a PNG larger than 50 megapixels or one that cannot be read, and the export message names the image and the reason.

## Reports in the Runtime

A navigation item can open a report in the Runtime, with the same preview, Print, and Export PDF… buttons. An action can open a report with parameter values, and a dashboard can show a report that follows its filters. A role needs read access to a report to open it or export it. [Roles and permissions](./roles) covers the grants a report needs.

When a report's queries declare parameters and the navigation item or action does not pass a value for each one, a dialog asks for them before the report runs. Each field matches the parameter's type, such as a number, a date, or a checkbox. The fields start with the values that were passed, then the report's defaults.

A required parameter needs a value, even when the action passes an empty one. **Cancel** leaves the report unrun with an **Enter parameters…** button, and **Change parameters…** above a finished report asks again. A dashboard passes its filter values and never asks.

## Next steps

- [Queries](./queries)
- [Expressions](../reference/expressions)
- [Dashboards](./dashboards)
