import { join } from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { screen, waitFor, within } from "@testing-library/react";
import { expect, it } from "vitest";
import type { Band, Report, ReportComponent } from "../../src/reports/types";
import { createTable, renderNewDocument, value } from "./helpers";
import { dialogMock } from "./setup";

type User = Awaited<ReturnType<typeof renderNewDocument>>;
type Config = Record<string, unknown> & {
  savedQueries: unknown[];
  reports: Report[];
  design: { forms: unknown[]; navigation: unknown[] } & Record<string, unknown>;
};

const LONG = { timeout: 20_000 };
const id = () => crypto.randomUUID();
const text = (v: string) => value("text", v);
const int = (v: number) => value("integer", v);

async function define(user: User, add: (config: Config) => void) {
  const config = await invoke<Config>("read_document_config", { windowLabel: "main" });
  add(config);
  await invoke("update_document_config", { windowLabel: "main", config });
  const archive = join(process.env.IXTABLE_STATE_DIR ?? "", `nested-${id()}.ixt`);
  dialogMock.save.mockResolvedValueOnce(archive);
  await user.click(screen.getByRole("button", { name: "Save project" }));
  await screen.findByText("Saved archive", {}, LONG);
  await user.click(screen.getByRole("button", { name: "Close project" }));
  dialogMock.open.mockResolvedValueOnce(archive);
  await user.click(await screen.findByRole("button", { name: /Open document/i }, LONG));
  await screen.findByText("Saved archive", {}, LONG);
}

async function seed() {
  const chain: [string, string, string][] = [
    ["customers", "", "name"],
    ["orders", "customer_id", "ref"],
    ["lines", "order_id", "item"],
    ["allocations", "line_id", "bin"],
  ];
  for (const [table, link, label] of chain)
    await createTable(table, [
      { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
      ...(link ? [{ name: link, declaredType: "INTEGER" }] : []),
      { name: label, declaredType: "TEXT" },
    ]);
  const insert = (table: string, values: [string, ReturnType<typeof value>][]) => ({
    op: "insert",
    table,
    values: values.map(([column, v]) => ({ column, value: v })),
  });
  await invoke("execute_write_batch", {
    windowLabel: "main",
    ops: [
      insert("customers", [
        ["id", int(1)],
        ["name", text("Acme")],
      ]),
      insert("orders", [
        ["id", int(1)],
        ["customer_id", int(1)],
        ["ref", text("SO-1")],
      ]),
      insert("orders", [
        ["id", int(2)],
        ["customer_id", int(1)],
        ["ref", text("SO-2")],
      ]),
      insert("lines", [
        ["id", int(1)],
        ["order_id", int(1)],
        ["item", text("Bolt")],
      ]),
      insert("lines", [
        ["id", int(2)],
        ["order_id", int(1)],
        ["item", text("Nut")],
      ]),
      insert("lines", [
        ["id", int(3)],
        ["order_id", int(2)],
        ["item", text("Gear")],
      ]),
      insert("allocations", [
        ["id", int(1)],
        ["line_id", int(1)],
        ["bin", text("Bin A1")],
      ]),
      insert("allocations", [
        ["id", int(2)],
        ["line_id", int(2)],
        ["bin", text("Bin B7")],
      ]),
    ],
  });
}

const placement = (row: number, columnSpan = 6) => ({ column: 1, row, columnSpan, rowSpan: 1 });

it("nests related lists three levels deep in Runtime", async () => {
  const user = await renderNewDocument();
  await seed();
  const ids = { customers: id(), orders: id(), lines: id(), allocations: id() };
  const form = (table: keyof typeof ids, column: string, child?: [keyof typeof ids, string]) => ({
    id: ids[table],
    name: table[0].toUpperCase() + table.slice(1),
    source: { kind: "table", table },
    modes: ["list", "detail", "create", "edit"],
    listColumns: [column],
    controls: [
      {
        id: id(),
        kind: "text",
        label: column,
        binding: { column },
        placement: placement(1),
      },
      ...(child
        ? [
            {
              id: id(),
              kind: "relatedList",
              label: child[0][0].toUpperCase() + child[0].slice(1),
              related: {
                table: child[0],
                foreignKey: child[1],
                parentColumn: "id",
                columns: [],
                formId: ids[child[0]],
              },
              placement: placement(2, 12),
            },
          ]
        : []),
    ],
  });
  await define(user, (config) => {
    config.design.forms.push(
      form("customers", "name", ["orders", "customer_id"]),
      form("orders", "ref", ["lines", "order_id"]),
      form("lines", "item", ["allocations", "line_id"]),
      form("allocations", "bin"),
    );
    config.design.navigation = [
      { id: id(), label: "Customers", kind: "form", targetId: ids.customers },
    ];
    config.design.startPage = null;
  });
  const issues = await invoke<Array<{ objectKind: string; severity: string; message: string }>>(
    "validate_document",
    { windowLabel: "main" },
  );
  expect(issues.filter((i) => i.objectKind === "form" && i.severity === "error")).toEqual([]);

  await user.click(screen.getByRole("button", { name: "Runtime" }));
  const nav = await screen.findByRole("navigation", { name: "Application navigation" }, LONG);
  await user.click(within(nav).getByRole("button", { name: "Customers" }));
  await user.click(await screen.findByRole("row", { name: "Open Acme" }, LONG));
  const orders = await screen.findByRole("region", { name: "Orders" }, LONG);
  await within(orders).findByRole("cell", { name: "SO-2" }, LONG);
  await user.click(within(orders).getByRole("button", { name: "Edit orders row 1" }));
  const order = await within(orders).findByRole("group", { name: "Orders record" }, LONG);
  const lines = await within(order).findByRole("region", { name: "Lines" }, LONG);
  await within(lines).findByRole("cell", { name: "Nut" }, LONG);
  expect(within(lines).queryByRole("cell", { name: "Gear" })).toBeNull();
  await user.click(within(lines).getByRole("button", { name: "Edit lines row 1" }));
  const line = await within(lines).findByRole("group", { name: "Lines record" }, LONG);
  const allocations = await within(line).findByRole("region", { name: "Allocations" }, LONG);
  await within(allocations).findByRole("cell", { name: "Bin A1" }, LONG);
  expect(within(allocations).queryByRole("cell", { name: "Bin B7" })).toBeNull();
}, 120_000);

const band = (height: number, components: ReportComponent[] = []): Band => ({
  height,
  keepTogether: false,
  components,
});
const field = (expression: string, y = 0, x = 0): ReportComponent => ({
  id: id(),
  kind: "field",
  expression,
  x,
  y,
  w: 200,
  h: 14,
});
const report = (name: string, datasetQueryId: string, detail: Band): Report => ({
  id: id(),
  name,
  datasetQueryId,
  params: {},
  page: {
    size: "A4",
    orientation: "portrait",
    margins: { top: 36, right: 36, bottom: 36, left: 36 },
  },
  bands: {
    reportHeader: band(0),
    pageHeader: band(0),
    groups: [],
    detail,
    pageFooter: band(0),
    reportFooter: band(0),
  },
});

it("prints each order's lines through a subreport", async () => {
  const user = await renderNewDocument();
  await seed();
  const ordersQuery = id();
  const linesQuery = id();
  await define(user, (config) => {
    config.savedQueries.push(
      { id: ordersQuery, name: "Orders", sql: "SELECT * FROM orders ORDER BY id" },
      { id: linesQuery, name: "Lines", sql: "SELECT * FROM lines ORDER BY id" },
    );
    const lines = report("Order lines", linesQuery, band(14, [field("'Item ' & record.item")]));
    const subreport: ReportComponent = {
      id: id(),
      kind: "subreport",
      reportId: lines.id,
      links: [{ child: "order_id", master: "id" }],
      x: 20,
      y: 14,
      w: 300,
      h: 20,
    };
    const invoices = report(
      "Invoices",
      ordersQuery,
      band(48, [field("'Order ' & record.ref"), subreport, field("'End of ' & record.ref", 34)]),
    );
    config.reports.push(lines, invoices);
  });
  const issues = await invoke<Array<{ objectKind: string }>>("validate_document", {
    windowLabel: "main",
  });
  expect(issues.filter((issue) => issue.objectKind === "report")).toEqual([]);

  await user.click(screen.getByRole("button", { name: "Reports" }));
  await user.click(await screen.findByRole("button", { name: "Invoices" }, LONG));
  await user.click(await screen.findByRole("button", { name: /^Subreport/ }, LONG));
  const target = screen.getByRole("combobox", { name: "Report" });
  expect(within(target).getByRole("option", { selected: true })).toHaveTextContent("Order lines");
  expect(screen.getByRole("textbox", { name: "Link 1 child field" })).toHaveValue("order_id");
  expect(screen.getByRole("combobox", { name: "Link 1 parent field" })).toHaveValue("id");

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  const printed = await waitFor(() => {
    const all = within(page)
      .getAllByText(/^(Order|Item|End of) /)
      .map((node) => node.textContent);
    expect(all.length).toBe(7);
    return all;
  }, LONG);
  expect(printed).toEqual([
    "Order SO-1",
    "Item Bolt",
    "Item Nut",
    "End of SO-1",
    "Order SO-2",
    "Item Gear",
    "End of SO-2",
  ]);
}, 120_000);
