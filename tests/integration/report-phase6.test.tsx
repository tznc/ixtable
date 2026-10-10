import { invoke } from "@tauri-apps/api/core";
import { screen, waitFor, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import { createTable, insertRow, renderNewDocument, value } from "./helpers";

const LONG = { timeout: 20_000 };
type User = ReturnType<typeof userEvent.setup>;

async function addComponent(user: User, band: string, kind: string) {
  await user.click(screen.getByRole("button", { name: new RegExp(`^${band} ·`) }));
  await user.click(screen.getByRole("button", { name: kind }));
}

async function typeInto(user: User, name: string, text: string) {
  const input = screen.getByLabelText(name);
  await user.clear(input);
  await user.type(input, text);
  await waitFor(() => expect(input).toHaveValue(text));
}

it("designs a running sum with conditional formatting and a chart, and previews them", async () => {
  const user = await renderNewDocument();
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "region", declaredType: "TEXT" },
    { name: "amount", declaredType: "REAL" },
  ]);
  const regions = ["East", "North", "West"];
  for (let i = 0; i < 6; i++)
    await insertRow("orders", [
      { column: "id", value: value("integer", i + 1) },
      { column: "region", value: value("text", regions[i % 3]) },
      { column: "amount", value: value("real", (i + 1) * 10) },
    ]);
  await invoke("save_query", {
    windowLabel: "main",
    id: null,
    name: "Orders",
    sql: "SELECT * FROM orders ORDER BY id",
    filterState: null,
  });

  await user.click(screen.getByRole("button", { name: "Reports" }));
  const create = await screen.findByRole("button", { name: "New report" }, LONG);
  await waitFor(() => expect(create).toBeEnabled(), LONG);
  await user.click(create);
  await screen.findByRole("textbox", { name: "Report name" }, LONG);
  await user.selectOptions(screen.getByRole("combobox", { name: "Dataset" }), "Query: Orders");

  await addComponent(user, "Detail", "Add field");
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Bound field" }, LONG),
    "amount",
  );
  await user.selectOptions(screen.getByRole("combobox", { name: "Running sum" }), "all");
  await user.click(screen.getByRole("button", { name: "Add rule" }));
  await typeInto(user, "Rule 1 condition", "value > 100");
  expect(screen.getByRole("checkbox", { name: "Rule 1 bold" })).toBeChecked();

  await addComponent(user, "Report footer", "Add chart");
  await typeInto(user, "Chart title", "Amount by region");
  await typeInto(user, "Category column", "region");
  await typeInto(user, "Value columns", "amount");
  await typeInto(user, "Value format", "0.0");

  await user.click(screen.getByRole("tab", { name: "Preview" }));
  const page = await screen.findByRole("img", { name: "Page 1 of 1" }, LONG);
  for (const total of ["10", "30", "60", "100"])
    expect(within(page).getByText(total)).toHaveAttribute("font-weight", "normal");
  for (const total of ["150", "210"])
    expect(within(page).getByText(total)).toHaveAttribute("font-weight", "bold");
  const chart = within(page).getByRole("img", { name: "Amount by region" });
  for (const label of ["East", "North", "West"])
    expect(within(chart).getByText(label)).toBeInTheDocument();
});
