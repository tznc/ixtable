# Report engine: deterministic layout and PDF output

Status: accepted. Covers PRD §15, the Phase 0 report pagination spike, and the
§26.5 report golden checks.

## Context

The PRD requires the report engine to paginate reports the same way on Windows,
macOS, and Linux. Preview, print, and PDF output all come from that one model. Report golden files must be deterministic within declared
tolerances. Reports must stay readable when printed without color.

Browser layout cannot give that guarantee. Text width depends on the installed
fonts, the font rasterizer, and the WebView engine. WebView2, WKWebView, and
WebKitGTK all differ, so a line that fits on one machine can wrap on another and
push a row onto the next page.

## Decision

Layout is a pure TypeScript function with no DOM access:

```ts
layoutReport(report, rows, { params, now, tables, assets }) → ReportDocument
```

It lives in `src/reports/engine/` and returns pages of positioned items in
points: `text` with pre-broken lines and baselines, `line`, `rect`, and `image`.
Every renderer draws those items as given. None of them measures or wraps text.

### Fixed font metrics

Text uses the PDF standard fonts Helvetica and Helvetica-Bold. Their advance
widths for every WinAnsi code come from the Adobe Core14 AFM files and ship as a
TypeScript table in `src/reports/engine/metrics.ts`. Wrapping sums integer
widths from that table, so the same string always breaks at the same place.

Characters outside WinAnsi fall back to two bundled TrueType fonts, in this
order: DejaVu Sans (Latin extensions, Greek, Cyrillic, symbols; Bitstream Vera
license) and Droid Sans Fallback (CJK ideographs and kana; Apache-2.0). They
live in `src-tauri/fonts/` and are compiled into the app, so output never
depends on the fonts a machine has installed. Their advance widths ship as
runs in `src/reports/engine/fallback-metrics.ts`, which a Rust test generates
from the font files and fails on when it is stale.

The fallback fonts never draw these code points, which print as `?`:
WinAnsi characters (Helvetica draws them), U+0590–U+08FF (Hebrew, Arabic,
Syriac, Thaana, N'Ko, Samaritan, Mandaic and their supplements), U+FB1D–U+FDFF
and U+FE70–U+FEFF (Hebrew and Arabic presentation forms), and lone surrogates.
These scripts need right-to-left order or contextual shaping, which the
engine doesn't do. Neither font has Hangul syllables, Thai, or Devanagari and
the other Indic scripts, so they print as `?` too. Lao and combining marks
are in DejaVu Sans and print, but unshaped: a mark sits at the pen position
instead of over its base. A character no font covers prints as `?`, in the
preview and in the PDF alike. Bold text
in a fallback font uses the regular glyphs at the same width; the PDF strokes
their outlines to darken them.

The on-screen and printed SVG ask for Helvetica, then Arial and Liberation Sans,
which share Helvetica's widths. Each line also sets `textLength` to the engine's
width, so a fallback font is stretched to the computed width instead of spilling
past its box.

### Pagination rules

- Rows sort by the group keys with a stable sort. Keys compare in a fixed order:
  null, booleans, numbers, then text by UTF-16 code unit. Locale never affects
  the order.
- The report header prints once, then for each group a header, the rows, and a
  footer, then the report footer. Group bands see the group's rows as `rows`.
- Page headers and footers print on every page. `page` and `pages` resolve after
  pagination, so `'Page ' & page & ' of ' & pages` is exact.
- A band moves to the next page when it doesn't fit. A band taller than the page
  body prints at the top of a page and overflows.
- `keepTogether` on a band with a table moves the band to the next page instead
  of splitting it. On a group header it also keeps the header on the page of the
  first row that follows.
- A band may hold one table. The table grows past its designed height, and
  components below it move down by the growth. When the table doesn't fit, it
  splits between rows and repeats its header row at the top of each new page.
- `pageBreakBefore` and `pageBreakAfter` on a band start a new page before or
  after each instance of it. A break never leaves a page empty: a break before
  is skipped at the top of a page, and a break after the last band adds no page.
  Page header and footer bands ignore them.
- A group with `newPage` starts every instance on a new page, like a break
  before its header. This holds even when the header band is empty.
- A group with `repeatHeader` prints its header band again at the top of every
  page the group continues on, below the page header, including the pages a
  split table continues on. The repeat sees the group's `rows` and `group`.
  It prints the header's non-table components at the band's designed height;
  a table in the header prints only once. Pagination reserves that height: a
  block that fits a page only without the repeated headers starts a page
  without them, and a `keepTogether` table that doesn't fit below them splits.
- A group with `resetPageNumber` starts every instance on a new page, so the
  designer shows its "starts a new page" option checked and disabled. It also
  restarts `groupPage` and `groupPages` there. They count the pages since the
  last such group start (or since the first page). Without any such group they
  equal `page` and `pages`, and `page` and `pages` always count the whole report.
- `pageBreakBefore`, `pageBreakAfter`, `newPage`, `repeatHeader`, and
  `resetPageNumber` default to off and are left out of the stored definition
  when off, so older reports lay out exactly as before. `keepTogether` is
  always stored.
- Page header and footer bands can't hold a table. The designer lists it as a
  problem and marks Add table `aria-disabled` with a visible reason on those
  bands. `reports::validate` returns a warning, not an error, so older
  documents that hold one still export. Layout leaves the table out and reports
  a diagnostic.
- Text boxes don't grow unless `canGrow` is set. Lines past the box height are
  dropped, and at least one line always shows.
- A text box with `canGrow` (static text, field, calculated) grows to the
  height of its wrapped text. Every component that starts at or below a growing
  box's designed bottom edge moves down by that box's growth plus the box's own
  shift, so stacked boxes push each other down. The band grows by the largest
  shift plus growth, and pagination uses the grown height. Text is measured
  with the band's row scope before pagination, so `page` and `pages` don't
  make a box grow. A repeated group header prints at its grown height. In a
  band with a table, and in page headers and footers, boxes keep their
  designed height; a table band reports a diagnostic, and the designer hides
  Can grow on page header and footer text.
- A band whose text grew splits across pages (`engine/split.ts`) unless
  `keepTogether` is set and the band fits on one page. It starts on the
  current page when its designed height fits there. Layout renders the whole
  band at its own origin, then cuts it into pieces that fit the remaining
  body: a cut falls between text lines, and an image or line that would cross
  it moves the cut above that item. Text keeps the lines that start in a
  piece; text frames, rectangles and vertical lines are clipped to it, so a
  bordered box shows its own part on each page. Each continuation starts at
  the body top below any repeated group headers, and components under the
  grown text follow its last piece. When not even one line fits an empty page
  body, the band is cut at the body bottom.

### Determinism guarantees and tolerances

The engine uses only integer metric sums and IEEE-754 arithmetic, which
JavaScript evaluates identically on every platform. Output coordinates round to
0.01 pt. The clock is an option, so `today()` and `now()` are fixed in tests.
Expression evaluation goes through `src/expr`, which already rounds arithmetic
to 15 significant digits.

The tolerance for layout goldens is zero: the same report, rows, and options
produce deep-equal page objects. The tolerance for PDF goldens is also zero: the
writer uses a fixed object order, uncompressed content streams, no random file
id, and a `/CreationDate` taken from the options. Studio passes the time the
preview loaded its data, so exporting the same preview twice writes identical
files.

On-screen rendering is not byte-exact, because the WebView draws the glyphs.
Positions and line breaks match the PDF exactly. Glyph shapes can differ when a
fallback font is used.

### PDF writer

`src/reports/pdf.ts` writes PDF 1.4 without dependencies. It references the
standard fonts with WinAnsiEncoding, so they are not embedded. Strings escape
`\`, `(`, and `)`, and bytes outside printable ASCII are written as octal
escapes, which keeps the file ASCII apart from image and font streams. Graphics
use gray levels only.

Text in a fallback font is written as a Type0 font with `Identity-H` encoding
and a `CIDFontType2` descendant (`CIDToGIDMap /Identity`), with a ToUnicode
CMap for copy and search. Each line splits into runs of one font, placed at the
engine's offsets. The preview draws the same runs as `tspan`s at the same
positions and widths.

JPEG images pass through unchanged with `DCTDecode`. Opaque non-interlaced
grayscale, RGB, and palette PNGs pass their IDAT data through with
`FlateDecode` and the PNG predictor. Every other PNG (an alpha channel, `tRNS`
transparency, interlacing, 16-bit samples) is decoded by Rust into 8-bit color
samples and, when any pixel is translucent, an 8-bit alpha image that the
writer attaches as the image's `/SMask`. 16-bit samples never pass through,
because `/BitsPerComponent 16` needs PDF 1.5. Rust rejects a PNG that
declares more than 50 megapixels before it allocates the pixel buffer, and
caps the decoder's own buffers at 64 MiB, so a small file that declares a huge
image can't exhaust memory. Other formats, and PNGs that are too large or
unreadable, print as a crossed placeholder box, and the export message names
each such image and the reason.

Export calls `prepare_report_pdf` (`src-tauri/src/report_pdf.rs`) only when a
report needs a fallback font or a decoded PNG. It is an async command, so the
subsetting, decoding and deflating run on a worker thread instead of the main
thread. It returns the font subsets
(`subsetter` crate, glyph ids renumbered in ascending order, subset tag hashed
from the glyphs) and the decoded images, all zlib-compressed by the pinned
`flate2` backend. Its output depends only on its input, so the same report
exports the same bytes on every OS. Reports that use neither write exactly the
bytes they wrote before.

Rust otherwise only stores and validates definitions (`reports::validate`) and
writes the finished bytes (`write_report_pdf`). Rust never evaluates
expressions.

### Parameters at run time

A report's parameters are the ones its dataset query declares, then its table
queries' (first declaration wins), with the report's `params` as defaults
ahead of the query's. When Runtime navigation or an `openReport` action opens a
report without a value for every parameter, `ReportRun` shows a dialog with one
input per parameter (number, date, date-time, checkbox, or text by logical type),
prefilled from the values passed, then the defaults. Required parameters must
have a value. Cancel leaves the report unrun with a button to enter parameters,
and "Change parameters…" reopens the dialog over a rendered report. Dashboards
and the Studio preview pass their own values and never prompt.

### Print

Print renders every page as an SVG sized in points into a container appended to
`<body>`, adds an `@page` rule with the page size and zero margin, and calls
`window.print()`. A print stylesheet hides the app and breaks after each page.

### Grayscale output

Styles carry gray levels, not colors: text gray, border width, and fill gray.
Table headers use a 0.9 gray fill with black text, which stays readable on a
monochrome printer. Chart marks are the one exception (see Charts).

### Running sums (Phase 6)

A field or calculated component with `runningSum` prints the sum of its
expression over every instance printed so far instead of the current value,
like Access's Running Sum property. In the detail band an instance is a row.
In a group header or footer it is one group instance, so
`sum(rows.amount)` with a running sum in a group footer gives a cumulative
subtotal. `"all"` never restarts. `"group"` restarts at each instance of the
enclosing group: the innermost group for the detail band, the next outer group
for a group band, and never for the outermost group's bands. The format
applies to the sum. Null adds nothing and booleans add 1 or 0. Any other
non-number prints `#Error` with a diagnostic, and the sum stays an error until
it restarts. Sums add to 15 significant digits like `src/expr`.

Sums are computed once while bands are flattened, in print order, so
pagination, repeated headers and split bands never count a row twice. In
report and page bands `runningSum` has no effect and the component prints its
plain value; the designer hides the option on page bands.

### Conditional formatting (Phase 6)

Static text, field and calculated components hold `conditions`: an ordered
list of rules, each a boolean expression and a style of bold, text gray and
fill gray. The first rule that holds applies its style on top of the
component's own. Rules see the band's scope plus `value`, the component's value
(after a running sum, before formatting; the text for static text). A rule
that fails to evaluate counts as false and adds a diagnostic. Styles stay gray
levels so conditional output prints on a monochrome printer. The style applies
before can-grow measuring, so a rule that makes text bold can make its box
grow.

### Charts (Phase 6)

A `chart` component draws bar, line, area, pie, donut and scatter charts with
the dashboard chart code (PRD §15, §16): `chartData`/`scatterData` from
`src/dashboards/data.ts` for series, `src/dashboards/charts/geometry.ts` for
bars, lines and areas, and the dashboard palette. Rows come from a saved query
(`queryId`, loaded with the report like table queries) or the band's `rows`,
so a chart in a group header charts that group. `xField`, `yFields`, `groupBy`
and `stacked` mean what `x`, `y`, `groupBy` and `stacked` mean on a dashboard
chart; `x` and `y` are a component's position, hence the different names.

`engine/chart.ts` turns that geometry into one `chart` item holding `path`
marks (move, line, cubic Bézier and close, in page points) and ordinary text
items, sized to the component. Pie and donut arcs are cubic Béziers of at most
a quarter turn each, since PDF has no arc operator. The preview draws the item
as an SVG group labelled with the chart title, and the PDF writes the paths
with RGB fill and stroke operators; text goes through the same font runs as
other text. A chart never grows and never splits: split pagination treats it
like an image and moves the whole chart to the next piece.

Series are drawn in the dashboard palette, in color in the preview and the
PDF. Labels, legend text and gridlines stay gray, and every chart with two or
more series has a legend; pie and donut legends list each slice's share.

## Deferred

PRD §15 defers nested subreports, report scripts, barcodes, label layouts, and
arbitrary HTML or CSS. This engine also leaves these for later:

- fonts other than Helvetica and the bundled fallbacks, italic text, a true
  bold fallback face, Hangul syllables, and scripts that need shaping or
  right-to-left layout
- can-grow text in bands with a table, can-shrink, and more than one table per
  band
- caching font subsets between exports
- image formats other than JPEG and PNG, which print as placeholders in the PDF
- color outside chart marks
- summary charts, and running sums in table columns
- content stream compression and PDF/A

## Proof

- `tests/unit/report-engine.test.ts`: golden layouts for multi-page pagination,
  group breaks, keep-together headers, a table that spans pages, and two-pass
  page numbers.
- `tests/unit/report-pdf.test.ts`: a byte-for-byte golden PDF, xref offset
  checks, string escaping, image passthrough, and two runs giving identical
  bytes.
- `tests/unit/report-grow.test.ts`: can-grow boxes push components and the band
  down, chain through stacked boxes, paginate by grown height, and repeat at
  grown height.
- `tests/unit/report-grow-split.test.ts`: a grown band taller than a page splits
  across pages without losing or repeating a line, a sibling beside it stays
  put, `keepTogether` moves a band that fits, and the cut and slice helpers.
- `tests/unit/report-unicode.test.ts`: fallback metrics, font runs, Type0 font
  objects and CID text, and SMask image objects.
- `src-tauri/src/report_pdf_tests.rs`: glyph coverage, deterministic subsets,
  the metrics table against the font files, PNG alpha splitting, and PNGs with
  huge declared sizes, truncated data, 16-bit samples, and interlacing.
- `tests/integration/report-pdf-resources.test.tsx`: exports Greek, Cyrillic,
  and CJK text and an RGBA PNG through the Rust command.
- `tests/integration/report-parameters.test.tsx`: the parameter prompt from
  Runtime navigation.
- `tests/unit/report-pagination.test.ts`: page assignments and positions for
  page breaks, new page per group, repeated group headers (also above a split
  table), group page numbers, and the diagnostic for a table in a page band.
- `tests/integration/golden/report-snapshots.test.tsx`: lays out every report
  of the CRM, Inventory, and Work orders golden apps from their template and
  seed rows, with a fixed clock and `TZ=UTC`, and compares the layout JSON and
  PDF bytes with `tests/fixtures/report-goldens/`. Run it with `UPDATE_GOLDENS=1` to
  rewrite the fixtures after an intended change. Asset ids, which are minted
  when the template is created, are replaced by `{{asset:N}}` in the layout
  JSON. `.gitattributes` keeps the fixtures out of line-ending conversion.
- `tests/integration/report.test.tsx`: builds a grouped report in the UI,
  previews it, prints it, and exports two identical PDFs through the Rust
  command. It also sets the pagination options in the designer and checks that
  Add table is blocked on page header and footer bands.
- `tests/unit/report-phase6.test.ts`: running sums over all rows, per group
  and across group footers, formats, nulls and errors; first-match
  conditional styles, conditions in grown text, failing conditions; charts of
  every type from band rows and saved queries, legends, "No data", a chart
  moving whole to the next page, RGB paths in the PDF, and the Bézier arc
  helpers.
- `tests/integration/report-phase6.test.tsx`: sets a running sum, a
  conditional bold rule and a bar chart in the designer and checks the
  preview.

## Audit log

- 2026-10-10: Added running sums, conditional formatting and charts (PRD
  Phase 6), with `tests/unit/report-phase6.test.ts` and
  `tests/integration/report-phase6.test.tsx` as proof.
- 2026-10-05: Said which options are omitted when off (`keepTogether` is
  always stored) and added the page-band table checks to the proof.
