import { readFileSync } from "node:fs";
import { join } from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { screen, waitFor, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import type { Band } from "../../src/reports/types";
import { createTable, insertRow, renderNewDocument, value } from "./helpers";
import { dialogMock } from "./setup";

const LONG = { timeout: 20_000 };
type User = ReturnType<typeof userEvent.setup>;

async function selectBand(user: User, label: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(`^${label} ·`) }));
}

async function addComponent(user: User, band: string, kind: string) {
  await selectBand(user, band);
  await user.click(screen.getByRole("button", { name: kind }));
}

async function setExpression(user: User, text: string) {
  const input = screen.getByRole("textbox", { name: "Expression" });
  await user.clear(input);
  await user.type(input, text);
  await waitFor(() => expect(input).toHaveValue(text));
}

async function openNewReport(user: User) {
  await user.click(screen.getByRole("button", { name: "Reports" }));
  const create = await screen.findByRole("button", { name: "New report" }, LONG);
  await waitFor(() => expect(create).toBeEnabled(), LONG);
  await user.click(create);
}

async function seedOrders() {
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "region", declaredType: "TEXT" },
    { name: "customer", declaredType: "TEXT" },
    { name: "amount", declaredType: "REAL" },
  ]);
  const regions = ["East", "North", "West"];
  for (let i = 0; i < 60; i++)
    await insertRow("orders", [
      { column: "id", value: value("integer", i + 1) },
      { column: "region", value: value("text", regions[i % 3]) },
      { column: "customer", value: value("text", `Customer ${i + 1}`) },
      { column: "amount", value: value("real", i + 1) },
    ]);
  await invoke("save_query", {
    windowLabel: "main",
    id: null,
    name: "Orders",
    sql: "SELECT * FROM orders",
    filterState: null,
  });
}

it("builds a grouped report, previews page 1 of N with totals, prints, and exports identical PDFs", async () => {
  const user = await renderNewDocument();
  await seedOrders();
  const archive = join(process.env.IXTABLE_STATE_DIR!, "orders.ixt");
  dialogMock.save.mockResolvedValueOnce(archive);
  await user.click(screen.getByRole("button", { name: "Save project" }));
  await screen.findByText("Saved archive", {}, LONG);
  await user.click(screen.getByRole("button", { name: "Close project" }));
  dialogMock.open.mockResolvedValueOnce(archive);
  await user.click(await screen.findByRole("button", { name: /Open document/i }, LONG));
  await screen.findByText("Saved archive", {}, LONG);
  await openNewReport(user);
  await screen.findByRole("textbox", { name: "Report name" }, LONG);

  await user.selectOptions(screen.getByRole("combobox", { name: "Dataset" }), "Query: Orders");
  await user.click(screen.getByRole("button", { name: "Add group" }));
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Group 1 field" }, LONG),
    "region",
  );
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "Group 1 expression" })).toHaveValue(
      "record.region",
    ),
  );

  await addComponent(user, "Report header", "Add text");
  const text = screen.getByRole("textbox", { name: "Text" });
  await user.clear(text);
  await user.type(text, "Sales by region");

  await addComponent(user, "Group 1 \\(record.region\\) header", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "region");

  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "customer");
  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(screen.getByRole("combobox", { name: "Bound field" }), "amount");

  await addComponent(user, "Group 1 \\(record.region\\) footer", "Add calculated");
  await setExpression(user, "'Subtotal ' & sum(rows.amount)");

  await addComponent(user, "Report footer", "Add calculated");
  await setExpression(user, "'Grand total ' & format(sum(rows.amount), '#,##0.00')");
  const field = screen.getByRole("button", { name: /^Calculated \['Grand total/ });
  field.focus();
  await user.keyboard("{Shift>}{ArrowRight}{/Shift}{ArrowDown}");
  await waitFor(() => expect(screen.getByRole("spinbutton", { name: "X" })).toHaveValue(10));
  expect(screen.getByRole("spinbutton", { name: "Y" })).toHaveValue(1);

  await addComponent(user, "Page footer", "Add calculated");
  await setExpression(user, "'Page ' & page & ' of ' & pages");

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 2" }, LONG);
  expect(within(page).getByText("Sales by region")).toBeInTheDocument();
  expect(within(page).getByText("East")).toBeInTheDocument();
  expect(within(page).getByText("Subtotal 590")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Next page" }));
  const last = await screen.findByRole("img", { name: "Page 2 of 2" });
  expect(within(last).getByText("Grand total 1,830.00")).toBeInTheDocument();
  expect(within(last).getByText("Page 2 of 2")).toBeInTheDocument();

  const print = vi.spyOn(window, "print").mockImplementation(() => undefined);
  await user.click(screen.getByRole("button", { name: "Print" }));
  await waitFor(() => expect(print).toHaveBeenCalledOnce());
  expect(screen.getAllByRole("img", { name: /^Printed page \d of 2$/ })).toHaveLength(2);

  const first = join(process.env.IXTABLE_STATE_DIR!, "report-a.pdf");
  const second = join(process.env.IXTABLE_STATE_DIR!, "report-b.pdf");
  dialogMock.save.mockResolvedValueOnce(first).mockResolvedValueOnce(second);
  await user.click(screen.getByRole("button", { name: "Export PDF…" }));
  expect(await screen.findByText(`Exported PDF to ${first}`, {}, LONG)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Export PDF…" }));
  expect(await screen.findByText(`Exported PDF to ${second}`, {}, LONG)).toBeInTheDocument();
  const a = readFileSync(first);
  expect(a.subarray(0, 8).toString("latin1")).toBe("%PDF-1.4");
  expect(a.toString("latin1")).toContain("(Grand total 1,830.00) Tj");
  expect(a.equals(readFileSync(second))).toBe(true);

  const config = await invoke<{ reports: Array<{ name: string; datasetQueryId: string }> }>(
    "read_document_config",
    { windowLabel: "main" },
  );
  expect(config.reports).toHaveLength(1);
  const issues = await invoke<Array<{ objectKind: string }>>("validate_document", {
    windowLabel: "main",
  });
  expect(issues.filter((issue) => issue.objectKind === "report")).toEqual([]);
}, 120_000);

it("renames, duplicates and deletes reports through the config store", async () => {
  const user = await renderNewDocument();
  await openNewReport(user);
  const name = await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.clear(name);
  await user.type(name, "Invoices");
  await user.click(screen.getByRole("button", { name: "Duplicate report" }));
  const copy = await screen.findByRole("button", { name: "Invoices copy" }, LONG);
  await waitFor(() => expect(copy).toHaveAttribute("aria-current", "true"), LONG);
  await user.click(screen.getByRole("button", { name: "Delete report" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Invoices copy" })).not.toBeInTheDocument(),
  );
  await waitFor(async () => {
    const config = await invoke<{ reports: Array<{ name: string }> }>("read_document_config", {
      windowLabel: "main",
    });
    expect(config.reports.map((r) => r.name)).toEqual(["Invoices"]);
  }, LONG);
});

it("sets pagination controls in the designer and keeps tables out of page bands", async () => {
  const user = await renderNewDocument();
  await seedOrders();
  const archive = join(process.env.IXTABLE_STATE_DIR!, "paging.ixt");
  dialogMock.save.mockResolvedValueOnce(archive);
  await user.click(screen.getByRole("button", { name: "Save project" }));
  await screen.findByText("Saved archive", {}, LONG);
  await user.click(screen.getByRole("button", { name: "Close project" }));
  dialogMock.open.mockResolvedValueOnce(archive);
  await user.click(await screen.findByRole("button", { name: /Open document/i }, LONG));
  await screen.findByText("Saved archive", {}, LONG);
  await openNewReport(user);
  await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.selectOptions(screen.getByRole("combobox", { name: "Dataset" }), "Query: Orders");
  await user.click(screen.getByRole("button", { name: "Add group" }));
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Group 1 field" }, LONG),
    "region",
  );
  await user.click(screen.getByRole("checkbox", { name: "Group 1 starts a new page" }));
  await user.click(screen.getByRole("checkbox", { name: "Group 1 starts a new page" }));
  expect(screen.getByRole("checkbox", { name: "Group 1 starts a new page" })).not.toBeChecked();
  await user.click(screen.getByRole("checkbox", { name: "Group 1 restarts group page numbers" }));
  const forcedNewPage = screen.getByRole("checkbox", { name: "Group 1 starts a new page" });
  expect(forcedNewPage).toBeChecked();
  expect(forcedNewPage).toBeDisabled();
  expect(forcedNewPage).toHaveAccessibleDescription("Always on while group page numbers restart.");
  await user.click(screen.getByRole("checkbox", { name: "Group 1 starts a new page" }));
  await selectBand(user, "Report header");
  await user.click(screen.getByRole("checkbox", { name: "Page break after" }));

  await selectBand(user, "Page footer");
  const addTable = screen.getByRole("button", { name: "Add table" });
  expect(addTable).toHaveAttribute("aria-disabled", "true");
  expect(addTable).not.toBeDisabled();
  expect(addTable).toHaveAccessibleDescription(
    "Tables and subreports are not supported in page headers or footers.",
  );
  expect(screen.getByRole("button", { name: "Add subreport" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await user.click(addTable);
  expect(screen.queryByRole("checkbox", { name: "Page break before" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Add calculated" }));
  await setExpression(user, "'Part ' & groupPage & ' of ' & groupPages & ' / ' & page");

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const first = await screen.findByRole("img", { name: "Page 1 of 4" }, LONG);
  expect(within(first).getByText("Part 1 of 1 / 1")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Next page" }));
  const second = await screen.findByRole("img", { name: "Page 2 of 4" });
  expect(within(second).getByText("Part 1 of 1 / 2")).toBeInTheDocument();

  const config = await invoke<{
    reports: Array<{
      bands: {
        reportHeader: { pageBreakAfter?: boolean };
        pageFooter: { components: Array<{ kind: string }> };
        groups: unknown[];
      };
    }>;
  }>("read_document_config", { windowLabel: "main" });
  const bands = config.reports[0].bands;
  expect(bands.reportHeader.pageBreakAfter).toBe(true);
  expect(bands.pageFooter.components.map((c) => c.kind)).toEqual(["calculated"]);
  expect(bands.groups[0]).toMatchObject({ resetPageNumber: true });
  expect((bands.groups[0] as { newPage?: boolean }).newPage).toBeFalsy();
  const issues = await invoke<Array<{ objectKind: string }>>("validate_document", {
    windowLabel: "main",
  });
  expect(issues.filter((issue) => issue.objectKind === "report")).toEqual([]);
});

it("lets a text box grow with its text in the designer and preview", async () => {
  const user = await renderNewDocument();
  await openNewReport(user);
  await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await addComponent(user, "Report header", "Add text");
  const text = screen.getByRole("textbox", { name: "Text" });
  await user.clear(text);
  await user.type(text, "Quarterly notes for the regional sales review meeting");
  const canGrow = screen.getByRole("checkbox", { name: "Can grow" });
  expect(canGrow).not.toBeChecked();

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const clipped = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  expect(within(clipped).getByText("Quarterly notes for the")).toBeInTheDocument();
  expect(within(clipped).queryByText("meeting")).toBeNull();

  await user.click(screen.getByRole("tab", { name: "Design" }));
  await user.click(screen.getByRole("button", { name: /^Text Quarterly/ }));
  await user.click(screen.getByRole("checkbox", { name: "Can grow" }));
  await waitFor(async () => {
    const config = await invoke<{ reports: Array<{ bands: { reportHeader: Band } }> }>(
      "read_document_config",
      { windowLabel: "main" },
    );
    expect(config.reports[0].bands.reportHeader.components[0]).toMatchObject({ canGrow: true });
  }, LONG);
  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const grown = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  await within(grown).findByText(/meeting$/, {}, LONG);
});
