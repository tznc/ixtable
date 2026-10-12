import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import type { Band, Report, ReportComponent } from "../../../src/reports/types";
import { captureDocument } from "../capture";
import {
  createTable,
  insert,
  LONG,
  openMode,
  renderNewDocument,
  saveAndReopen,
  seedSales,
  type User,
} from "./fixtures";

type Config = Record<string, unknown> & {
  savedQueries: unknown[];
  reports: Report[];
  design: { forms: unknown[]; navigation: unknown[] } & Record<string, unknown>;
};

const id = () => crypto.randomUUID();

async function define(user: User, name: string, add: (config: Config) => void) {
  const config = await invoke<Config>("read_document_config", { windowLabel: "main" });
  add(config);
  await invoke("update_document_config", { windowLabel: "main", config });
  await saveAndReopen(user, name);
}

/** seedSales plus stock allocations of each order item: customers → orders → items → allocations. */
async function seedAllocations() {
  await seedSales();
  await createTable(
    "allocations",
    [
      { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
      { name: "order_item_id", declaredType: "INTEGER" },
      { name: "bin", declaredType: "TEXT" },
      { name: "quantity", declaredType: "INTEGER" },
    ],
    [{ columns: ["order_item_id"], targetTable: "order_items", targetColumns: ["id"] }],
  );
  const rows = [
    [1, 1, "Aisle 3 / Bin A1", 1],
    [2, 1, "Aisle 3 / Bin A2", 1],
    [3, 2, "Aisle 7 / Bin C4", 1],
  ] as const;
  for (const [rowId, order_item_id, bin, quantity] of rows)
    await insert("allocations", { id: rowId, order_item_id, bin, quantity });
}

const placement = (row: number, columnSpan = 6) => ({ column: 1, row, columnSpan, rowSpan: 1 });

it("nests related lists three levels deep in the designer and Runtime", async () => {
  const user = await renderNewDocument();
  await seedAllocations();
  const ids = { customers: id(), orders: id(), order_items: id(), allocations: id() };
  type Table = keyof typeof ids;
  const names: Record<Table, string> = {
    customers: "Customers",
    orders: "Orders",
    order_items: "Order items",
    allocations: "Allocations",
  };
  const form = (table: Table, columns: string[], child?: [Table, string]) => ({
    id: ids[table],
    name: names[table],
    source: { kind: "table", table },
    modes: ["list", "detail", "create", "edit"],
    listColumns: columns,
    controls: [
      ...columns.map((column, i) => ({
        id: id(),
        kind: "text",
        label: column[0].toUpperCase() + column.slice(1),
        binding: { column },
        placement: { column: 1 + i * 6, row: 1, columnSpan: 6, rowSpan: 1 },
      })),
      ...(child
        ? [
            {
              id: id(),
              kind: "relatedList",
              label: names[child[0]],
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
  await define(user, "nested-forms.ixt", (config) => {
    config.design.forms.push(
      form("customers", ["name", "city"], ["orders", "customer_id"]),
      form("orders", ["status", "amount"], ["order_items", "order_id"]),
      form("order_items", ["product", "quantity"], ["allocations", "order_item_id"]),
      form("allocations", ["bin", "quantity"]),
    );
    config.design.navigation = [
      { id: id(), label: "Customers", kind: "form", targetId: ids.customers },
    ];
    config.design.startPage = null;
  });

  await openMode(user, "Design");
  const forms = await screen.findByRole("region", { name: "Forms" }, LONG);
  await user.click(await within(forms).findByRole("button", { name: /^Order items/ }, LONG));
  await user.click(await screen.findByRole("group", { name: "Allocations" }, LONG));
  const properties = screen.getByRole("complementary", { name: "Properties" });
  const child = within(properties).getByRole("combobox", { name: "Child form" });
  const offered = within(child)
    .getAllByRole("option")
    .map((option) => option.textContent);
  expect(offered).toEqual(["Generated from the table", "Main form", "Allocations"]);
  await captureDocument(document, {
    name: "nesting-01-child-form-picker",
    expectations: [
      "The Order items form is open in the designer with its Allocations related list selected.",
      "The Child form picker offers Main form and Allocations only: Customers and Orders would loop back, so they are left out.",
    ],
  });

  await openMode(user, "Runtime");
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Customers" }));
  await user.click(await within(page).findByRole("row", { name: "Open Northstar Goods" }, LONG));
  const orders = await screen.findByRole("region", { name: "Orders" }, LONG);
  await within(orders).findByRole("cell", { name: "340" }, LONG);
  await user.click(within(orders).getByRole("button", { name: "Edit orders row 1" }));
  const order = await within(orders).findByRole("group", { name: "Orders record" }, LONG);
  const items = await within(order).findByRole("region", { name: "Order items" }, LONG);
  await within(items).findByRole("cell", { name: "Oak serving board" }, LONG);
  await user.click(within(items).getByRole("button", { name: "Edit order items row 1" }));
  const item = await within(items).findByRole("group", { name: "Order items record" }, LONG);
  const allocations = await within(item).findByRole("region", { name: "Allocations" }, LONG);
  await within(allocations).findByRole("cell", { name: "Aisle 3 / Bin A2" }, LONG);
  expect(within(allocations).queryByRole("cell", { name: "Aisle 7 / Bin C4" })).toBeNull();
  await captureDocument(document, {
    name: "nesting-02-runtime-three-levels",
    expectations: [
      "Customer Northstar Goods shows its orders; order 1 is open for editing inside the list.",
      "Inside order 1, its order items list has the Ceramic pour-over set open for editing.",
      "Inside that item, the Allocations list shows bins A1 and A2 only.",
    ],
  });
}, 180_000);

const band = (height: number, components: ReportComponent[] = []): Band => ({
  height,
  keepTogether: false,
  components,
});
const text = (
  expression: string,
  x: number,
  y: number,
  w = 200,
  bold = false,
): ReportComponent => ({
  id: id(),
  kind: "calculated",
  expression,
  x,
  y,
  w,
  h: 14,
  style: bold ? { bold: true } : undefined,
});
const report = (name: string, datasetQueryId: string, bands: Partial<Report["bands"]>): Report => ({
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
    detail: band(0),
    pageFooter: band(0),
    reportFooter: band(0),
    ...bands,
  },
});

it("prints each order's items through a subreport", async () => {
  const user = await renderNewDocument();
  await seedSales();
  const ordersQuery = id();
  const itemsQuery = id();
  await define(user, "invoices.ixt", (config) => {
    config.savedQueries.push(
      {
        id: ordersQuery,
        name: "Orders with customers",
        sql: "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.id",
      },
      { id: itemsQuery, name: "Order items", sql: "SELECT * FROM order_items ORDER BY id" },
    );
    const items = report("Order items", itemsQuery, {
      reportHeader: band(16, [text("'Items of order ' & parent.id", 0, 0, 300, true)]),
      detail: band(14, [text("record.product", 0, 0), text("'x ' & record.quantity", 210, 0, 60)]),
      reportFooter: band(16, [text("'Pieces: ' & sum(rows.quantity)", 0, 0)]),
    });
    const subreport: ReportComponent = {
      id: id(),
      kind: "subreport",
      reportId: items.id,
      links: [{ child: "order_id", master: "id" }],
      x: 24,
      y: 18,
      w: 400,
      h: 30,
    };
    const invoices = report("Invoices", ordersQuery, {
      reportHeader: band(30, [
        { ...text("'Invoices'", 0, 0, 300, true), h: 20, style: { bold: true, fontSize: 16 } },
      ]),
      detail: band(72, [
        text("'Order ' & record.id & ' · ' & record.customer", 0, 0, 300, true),
        text("format(record.amount, '#,##0.00')", 420, 0, 100),
        subreport,
        text("'Status: ' & record.status", 0, 52),
      ]),
    });
    config.reports.push(items, invoices);
  });

  await openMode(user, "Reports");
  await user.click(await screen.findByRole("button", { name: "Invoices" }, LONG));
  await user.click(await screen.findByRole("button", { name: /^Subreport/ }, LONG));
  const target = screen.getByRole("combobox", { name: "Report" });
  expect(within(target).getByRole("option", { selected: true })).toHaveTextContent("Order items");
  expect(screen.getByRole("textbox", { name: "Link 1 child field" })).toHaveValue("order_id");
  await captureDocument(document, {
    name: "nesting-03-subreport-designer",
    expectations: [
      "The Invoices detail band holds an order line, a Subreport box, and a status line below it.",
      "The properties panel shows the Subreport section printing Order items, linked by order_id to id.",
    ],
  });

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  await waitFor(() => expect(within(page).getByText("Linen napkins")).toBeInTheDocument(), LONG);
  expect(within(page).getAllByText(/^Items of order /)).toHaveLength(2);
  await captureDocument(document, {
    name: "nesting-04-subreport-preview",
    expectations: [
      "Order 1 lists the pour-over set and serving board with 'Pieces: 3'; order 2 lists linen napkins with 'Pieces: 6'.",
      "Orders 3, 4 and 5 have no items, so no subreport prints and their status line follows directly.",
      "Item lines are indented under each order and the status line sits below the items.",
    ],
  });
}, 180_000);
