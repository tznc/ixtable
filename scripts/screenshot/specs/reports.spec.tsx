import { readFileSync } from "node:fs";
import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { newReport } from "../../../src/reports/model";
import { captureDocument } from "../capture";
import {
  dialogMock,
  LONG,
  openMode,
  renderNewDocument,
  saveAndReopen,
  saveQuery,
  seedSales,
  tempPath,
  type User,
} from "./fixtures";

async function addComponent(user: User, band: string, kind: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(`^${band} ·`) }));
  await user.click(screen.getByRole("button", { name: kind }));
}

async function setExpression(user: User, text: string) {
  const input = screen.getByRole("textbox", { name: "Expression" });
  await user.clear(input);
  await user.type(input, text);
  await waitFor(() => expect(input).toHaveValue(text));
}

it("designs a grouped report, previews it, prints, and exports a PDF", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await saveQuery(
    "Orders with customers",
    "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.status, o.id",
  );
  await saveAndReopen(user, "report.ixt");

  await openMode(user, "Reports");
  const create = await screen.findByRole("button", { name: "New report" }, LONG);
  await waitFor(() => expect(create).toBeEnabled(), LONG);
  await user.click(create);
  const name = await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.clear(name);
  await user.type(name, "Orders by status");
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Dataset" }),
    "Query: Orders with customers",
  );
  await user.click(screen.getByRole("button", { name: "Add group" }));
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Group 1 field" }, LONG),
    "status",
  );
  await addComponent(user, "Report header", "Add text");
  const text = screen.getByRole("textbox", { name: "Text" });
  await user.clear(text);
  await user.type(text, "Orders by status");
  await addComponent(user, "Group 1 \\(record.status\\) header", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "status");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "customer");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "amount");
  const detailBand = screen.getByRole("group", { name: "Detail band" });
  const amount = within(detailBand).getByRole("button", { name: /amount/ });
  amount.focus();
  await user.keyboard("{Shift>}{ArrowRight}{ArrowRight}{ArrowRight}{ArrowRight}{/Shift}");
  await addComponent(user, "Group 1 \\(record.status\\) footer", "Add calculated");
  await setExpression(user, "'Subtotal ' & format(sum(rows.amount), '#,##0.00')");
  await addComponent(user, "Report footer", "Add calculated");
  await setExpression(user, "'Grand total ' & format(sum(rows.amount), '#,##0.00')");
  await addComponent(user, "Page footer", "Add calculated");
  await setExpression(user, "'Page ' & page & ' of ' & pages");
  await captureDocument(document, {
    name: "reports-01-designer",
    expectations: [
      "The band canvas shows report header, group header/footer, detail, and footers with their components.",
      "The selected page-footer calculated field and its expression appear in the properties panel.",
    ],
  });

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  expect(within(page).getByText("Orders by status")).toBeInTheDocument();
  expect(within(page).getByText("Grand total 965.50")).toBeInTheDocument();
  await captureDocument(document, {
    name: "reports-02-preview",
    expectations: [
      "The preview page shows the title, one group per status with customer rows and subtotals.",
      "The grand total 965.50 and 'Page 1 of 1' footer are visible on the page.",
    ],
  });

  const print = vi.spyOn(window, "print").mockImplementation(() => undefined);
  await user.click(screen.getByRole("button", { name: "Print" }));
  await waitFor(() => expect(print).toHaveBeenCalledOnce());
  const pdf = tempPath("orders-by-status.pdf");
  dialogMock.save.mockResolvedValueOnce(pdf);
  await user.click(screen.getByRole("button", { name: "Export PDF…" }));
  expect(await screen.findByText(`Exported PDF to ${pdf}`, {}, LONG)).toBeInTheDocument();
  expect(readFileSync(pdf).subarray(0, 5).toString("latin1")).toBe("%PDF-");
  await captureDocument(document, {
    name: "reports-03-exported",
    expectations: ["The preview toolbar confirms the PDF export path after Print and Export."],
  });
});

it("sets page breaks and group paging, and repeats a group header across pages", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await saveQuery(
    "Orders with customers",
    "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.status, o.id",
  );
  await saveAndReopen(user, "paging.ixt");
  await openMode(user, "Reports");
  const create = await screen.findByRole("button", { name: "New report" }, LONG);
  await waitFor(() => expect(create).toBeEnabled(), LONG);
  await user.click(create);
  const name = await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.clear(name);
  await user.type(name, "Orders paged by status");
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Dataset" }),
    "Query: Orders with customers",
  );
  await user.click(screen.getByRole("button", { name: "Add group" }));
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Group 1 field" }, LONG),
    "status",
  );
  await user.click(screen.getByRole("checkbox", { name: "Group 1 repeats header on each page" }));
  await user.click(screen.getByRole("checkbox", { name: "Group 1 restarts group page numbers" }));
  const forced = screen.getByRole("checkbox", { name: "Group 1 starts a new page" });
  expect(forced).toBeChecked();
  expect(forced).toBeDisabled();
  expect(forced).toHaveAccessibleDescription("Always on while group page numbers restart.");
  await addComponent(user, "Report header", "Add text");
  const text = screen.getByRole("textbox", { name: "Text" });
  await user.clear(text);
  await user.type(text, "Orders paged by status");
  await user.click(screen.getByRole("button", { name: /^Report header ·/ }));
  await user.click(screen.getByRole("checkbox", { name: "Page break after" }));
  expect(screen.getByRole("checkbox", { name: "Page break after" })).toBeChecked();
  await addComponent(user, "Group 1 \\(record.status\\) header", "Add calculated");
  await setExpression(user, "'Status: ' & record.status");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "customer");
  await user.click(screen.getByRole("button", { name: /^Detail ·/ }));
  const height = screen.getByRole("spinbutton", { name: "Band height" });
  await user.tripleClick(height);
  await user.paste("450");
  await waitFor(() => expect(height).toHaveValue(450));
  await user.tab();
  await addComponent(user, "Page footer", "Add calculated");
  await setExpression(user, "'Part ' & groupPage & ' of ' & groupPages & ' / page ' & page");
  await user.click(screen.getByRole("button", { name: /^Group 1 \(record.status\) header ·/ }));
  await captureDocument(document, {
    name: "reports-04-paging-settings",
    expectations: [
      "Group 1 has 'repeats header on each page' and 'restarts group page numbers' checked.",
      "'Group 1 starts a new page' is checked and disabled with the hint 'Always on while group page numbers restart.'",
    ],
  });

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const first = await screen.findByRole("img", { name: /^Page 1 of \d+$/ }, LONG);
  const total = Number(first.getAttribute("aria-label")?.split(" of ")[1]);
  expect(total).toBeGreaterThan(2);
  const pages: string[] = [];
  for (let n = 1; n <= total; n++) {
    if (n > 1) await user.click(screen.getByRole("button", { name: "Next page" }));
    const page = await screen.findByRole("img", { name: `Page ${n} of ${total}` }, LONG);
    pages.push(page.textContent ?? "");
    if (/Status: open/.test(page.textContent ?? "") && /Part 2 of/.test(page.textContent ?? ""))
      break;
  }
  const shown = pages.at(-1) ?? "";
  expect(shown, pages.join(" | ")).toMatch(/Status: open/);
  expect(shown).toMatch(/Part 2 of 2/);
  await captureDocument(document, {
    name: "reports-05-repeated-group-header",
    expectations: [
      "The preview page repeats the 'Status: open' group header above the continued detail row.",
      "The page footer reads 'Part 2 of 2' for the open group.",
    ],
  });
});

it("prompts for parameters in the Runtime and prints grown Unicode text", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await saveQuery(
    "Orders by status",
    "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.status = $status AND o.amount >= $minimum ORDER BY o.id",
    [
      { name: "status", logicalType: "text", required: true },
      { name: "minimum", logicalType: "number", defaultValue: 0 },
    ],
  );
  const config = await invoke<
    Record<string, unknown> & {
      savedQueries: { id: string; name: string }[];
      design: Record<string, unknown>;
    }
  >("read_document_config", { windowLabel: "main" });
  const report = newReport("Orders for review");
  report.datasetQueryId = config.savedQueries.find((q) => q.name === "Orders by status")?.id;
  report.params = { status: "open" };
  report.bands.reportHeader.components = [
    {
      id: crypto.randomUUID(),
      kind: "staticText",
      text: "Revue des commandes · Обзор заказов · Ανασκόπηση παραγγελιών · 订单审核 → ✓ Notes for the review meeting grow with their text.",
      x: 0,
      y: 0,
      w: 260,
      h: 16,
      canGrow: true,
      style: { borderWidth: 0.5 },
    },
    {
      id: crypto.randomUUID(),
      kind: "staticText",
      text: "Customer · Amount",
      x: 0,
      y: 22,
      w: 260,
      h: 16,
      style: { bold: true },
    },
  ];
  report.bands.detail.components = [
    {
      id: crypto.randomUUID(),
      kind: "field",
      expression: "record.customer & ' · ' & record.amount",
      x: 0,
      y: 0,
      w: 260,
      h: 16,
    },
  ];
  await invoke("update_document_config", {
    windowLabel: "main",
    config: {
      ...config,
      reports: [report],
      design: {
        ...config.design,
        navigation: [
          { id: crypto.randomUUID(), label: "Review", kind: "report", targetId: report.id },
        ],
      },
    },
  });
  await saveAndReopen(user, "review.ixt");
  await openMode(user, "Runtime");
  const nav = await screen.findByRole("navigation", { name: "Application navigation" }, LONG);
  await user.click(within(nav).getByRole("button", { name: "Review" }));
  const dialog = await screen.findByRole("dialog", { name: "Orders for review parameters" }, LONG);
  expect(within(dialog).getByRole("textbox", { name: /status/ })).toHaveValue("open");
  await captureDocument(document, {
    name: "reports-06-parameter-prompt",
    expectations: [
      "A dialog titled 'Orders for review' asks for status (prefilled 'open', required) and minimum (0).",
      "The dialog has Cancel and Run report buttons and no report page is shown behind it.",
    ],
  });
  await user.click(within(dialog).getByRole("button", { name: "Run report" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  await within(page).findByText(/Northstar Goods/, {}, LONG);
  expect(page.textContent).toContain("订单审核");
  expect(page.textContent).not.toContain("Harbor");
  await captureDocument(document, {
    name: "reports-07-unicode-can-grow",
    expectations: [
      "The header text with French, Russian, Greek and Chinese wraps over several lines inside a grown bordered box.",
      "The bold 'Customer · Amount' heading and the open orders sit below the grown box without overlapping it.",
    ],
  });
});

it("adds a running sum with conditional formatting and charts, and previews them", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await saveQuery(
    "Orders with customers",
    "SELECT o.id, c.name AS customer, o.status, o.amount FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.status, o.id",
  );
  await saveAndReopen(user, "phase6.ixt");
  await openMode(user, "Reports");
  const create = await screen.findByRole("button", { name: "New report" }, LONG);
  await waitFor(() => expect(create).toBeEnabled(), LONG);
  await user.click(create);
  const name = await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.clear(name);
  await user.type(name, "Running totals");
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Dataset" }),
    "Query: Orders with customers",
  );
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "customer");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "status");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "amount");
  await user.selectOptions(screen.getByRole("combobox", { name: "Running sum" }), "all");
  await typeInto(user, "Format", "#,##0.00");
  await user.click(screen.getByRole("button", { name: "Add rule" }));
  await typeInto(user, "Rule 1 condition", "value > 500");
  await user.selectOptions(screen.getByRole("combobox", { name: "Rule 1 fill" }), "Light gray");
  expect(screen.getByRole("checkbox", { name: "Rule 1 bold" })).toBeChecked();
  await captureDocument(document, {
    name: "reports-08-running-sum-rule",
    expectations: [
      "The detail band holds customer, status and amount fields; the amount field is selected.",
      "The properties panel shows Running sum 'Over all' and conditional formatting rule 1 'value > 500' with Bold checked and a light gray fill.",
    ],
  });

  await addComponent(user, "Report footer", "Add chart");
  await typeInto(user, "Chart title", "Amount by status");
  await typeInto(user, "Category column", "status");
  await typeInto(user, "Value columns", "amount");
  await setPoint(user, "Width", 255);
  await addComponent(user, "Report footer", "Add chart");
  await user.selectOptions(screen.getByRole("combobox", { name: "Chart type" }), "donut");
  await typeInto(user, "Chart title", "Amount by customer");
  await typeInto(user, "Category column", "customer");
  await typeInto(user, "Value column", "amount");
  await setPoint(user, "Width", 260);
  await setPoint(user, "X", 263);
  await captureDocument(document, {
    name: "reports-09-chart-designer",
    expectations: [
      "The report footer band holds two chart components side by side, labelled 'bar chart: amount' and 'donut chart: amount'.",
      "The properties panel shows the chart settings: type Donut, title 'Amount by customer', category column customer, value column amount.",
    ],
  });

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  for (const total of ["20.00", "140.00", "215.50"])
    expect(within(page).getByText(total)).toHaveAttribute("font-weight", "normal");
  for (const total of ["555.50", "965.50"])
    expect(within(page).getByText(total)).toHaveAttribute("font-weight", "bold");
  const bars = within(page).getByRole("img", { name: "Amount by status" });
  for (const label of ["cancelled", "open", "shipped"])
    expect(within(bars).getByText(label)).toBeInTheDocument();
  const donut = within(page).getByRole("img", { name: "Amount by customer" });
  expect(within(donut).getByText("Northstar Goods (48%)")).toBeInTheDocument();
  await captureDocument(document, {
    name: "reports-10-running-sum-charts-preview",
    expectations: [
      "The amount column is a running total (20.00, 140.00, 215.50, 555.50, 965.50); the last two are bold on a light gray fill.",
      "Below the rows, a blue bar chart 'Amount by status' with bars for cancelled, open and shipped sits beside a donut 'Amount by customer' with a legend of three customers and their shares.",
    ],
  });
});

async function typeInto(user: User, label: string, text: string) {
  const input = screen.getByLabelText(label);
  await user.clear(input);
  await user.type(input, text);
  await waitFor(() => expect(input).toHaveValue(text));
}

async function setPoint(user: User, label: string, value: number) {
  const input = screen.getByRole("spinbutton", { name: label });
  await user.clear(input);
  await user.type(input, String(value));
  await waitFor(() => expect(input).toHaveValue(value));
}
