import { screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { createTable, insertRow, readPage, renderNewDocument, value } from "./helpers";

const LONG = { timeout: 20_000 };

async function seedOrders() {
  const user = await renderNewDocument();
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", nullable: false, primaryKeyPosition: 1 },
    { name: "name", declaredType: "TEXT" },
    { name: "region", declaredType: "TEXT" },
    { name: "qty", declaredType: "INTEGER" },
  ]);
  for (const [id, name, region, qty] of [
    [1, "Acme Ltd", "north", 2],
    [2, "Bolt Ltd", "south", 3],
    [3, "Core Inc", "north", 10],
  ] as const)
    await insertRow("orders", [
      { column: "id", value: value("integer", id) },
      { column: "name", value: value("text", name) },
      { column: "region", value: value("text", region) },
      { column: "qty", value: value("integer", qty) },
    ]);
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByRole("textbox", { name: "qty, row 3" }, LONG);
  return user;
}

const cellValues = (column: string) =>
  screen
    .getAllByRole("textbox", { name: new RegExp(`^${column}, row \\d+$`) })
    .map((input) => (input as HTMLInputElement).value);

it("filters by selection, excludes a selection, and clears filters", async () => {
  const user = await seedOrders();
  await user.click(screen.getByRole("textbox", { name: "region, row 1" }));
  await user.click(screen.getByRole("button", { name: "Filter by selection" }));
  await waitFor(() => expect(cellValues("name")).toEqual(["Acme Ltd", "Core Inc"]), LONG);
  const chips = screen.getByRole("list", { name: "Active filters" });
  expect(within(chips).getByText("region = north")).toBeInTheDocument();

  await user.click(screen.getByRole("textbox", { name: "name, row 1" }));
  await user.click(screen.getByRole("button", { name: "Filter excluding selection" }));
  await waitFor(() => expect(cellValues("name")).toEqual(["Core Inc"]), LONG);

  await user.click(screen.getByRole("button", { name: "Remove filter region = north" }));
  await waitFor(() => expect(cellValues("name")).toEqual(["Bolt Ltd", "Core Inc"]), LONG);
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  await waitFor(() => expect(cellValues("name")).toHaveLength(3), LONG);
});

it("finds the next match and replaces every match in one batch", async () => {
  const user = await seedOrders();
  await user.click(screen.getByRole("button", { name: "Find" }));
  const panel = screen.getByRole("dialog", { name: "Find and replace" });
  await user.type(within(panel).getByRole("textbox", { name: "Find" }), "ltd");
  await user.click(within(panel).getByRole("button", { name: "Find next" }));
  await waitFor(
    () => expect(screen.getByRole("textbox", { name: "name, row 1" })).toHaveFocus(),
    LONG,
  );
  await user.click(within(panel).getByRole("button", { name: "Find next" }));
  await waitFor(
    () => expect(screen.getByRole("textbox", { name: "name, row 2" })).toHaveFocus(),
    LONG,
  );

  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  await user.type(within(panel).getByRole("textbox", { name: "Replace with" }), "LLC");
  await user.click(within(panel).getByRole("button", { name: "Replace all" }));
  expect(await within(panel).findByRole("status", {}, LONG)).toHaveTextContent(
    "Replaced 2 matches in 2 records.",
  );
  expect(confirm).toHaveBeenCalledWith("Replace 2 matches in 2 records? This cannot be undone.");
  const page = await readPage("orders");
  expect(page.rows.map((row) => row[1])).toEqual([
    value("text", "Acme LLC"),
    value("text", "Bolt LLC"),
    value("text", "Core Inc"),
  ]);
  confirm.mockRestore();
});

it("shows a totals row computed over every filtered record", async () => {
  const user = await seedOrders();
  await user.click(screen.getByRole("button", { name: "Totals" }));
  const total = await screen.findByRole("combobox", { name: "Total for qty" }, LONG);
  await user.selectOptions(total, "sum");
  const cell = () => screen.getByRole("combobox", { name: "Total for qty" }).closest("td")!;
  await waitFor(() => expect(within(cell()).getByRole("status")).toHaveTextContent("15"), LONG);

  await user.click(screen.getByRole("textbox", { name: "region, row 1" }));
  await user.click(screen.getByRole("button", { name: "Filter by selection" }));
  await waitFor(() => expect(within(cell()).getByRole("status")).toHaveTextContent("12"), LONG);
  expect(screen.getByRole("button", { name: "Totals" })).toHaveAttribute("aria-pressed", "true");
});

it("hides, shows, freezes and unfreezes columns", async () => {
  const user = await seedOrders();
  await user.click(screen.getByRole("button", { name: "Column options for region" }));
  await user.click(await screen.findByRole("menuitem", { name: "Hide column" }));
  await waitFor(
    () => expect(screen.queryByRole("textbox", { name: "region, row 1" })).toBeNull(),
    LONG,
  );
  await user.click(screen.getByRole("button", { name: "1 hidden" }));
  await user.click(await screen.findByRole("menuitem", { name: "Show all columns" }));
  await screen.findByRole("textbox", { name: "region, row 1" }, LONG);

  await user.click(screen.getByRole("button", { name: "Column options for name" }));
  await user.click(await screen.findByRole("menuitem", { name: "Freeze through this column" }));
  const nameHeader = () =>
    screen.getByRole("button", { name: "Column options for name" }).closest("th")!;
  const qtyHeader = () =>
    screen.getByRole("button", { name: "Column options for qty" }).closest("th")!;
  await waitFor(() => expect(nameHeader()).toHaveClass("frozen"), LONG);
  expect(qtyHeader()).not.toHaveClass("frozen");
  await user.click(screen.getByRole("button", { name: "Unfreeze columns" }));
  await waitFor(() => expect(nameHeader()).not.toHaveClass("frozen"), LONG);
});

it("pastes a spreadsheet block over existing records and as new records", async () => {
  const user = await seedOrders();
  await user.click(screen.getByRole("textbox", { name: "qty, row 1" }));
  await user.paste("20\r\n30\r\n");
  expect(await screen.findByRole("status", {}, LONG)).toHaveTextContent(
    "Pasted into 2 records.",
  );
  await waitFor(async () => {
    const page = await readPage("orders");
    expect(page.rows.map((row) => row[3])).toEqual([
      value("integer", 20),
      value("integer", 30),
      value("integer", 10),
    ]);
  }, LONG);

  await user.click(screen.getByRole("textbox", { name: "New id" }));
  await user.paste('4\t"Delta, ""Co"""\tsouth\t7\r\n5\tEcho\t\t1\r\n');
  await waitFor(async () => expect((await readPage("orders")).total).toBe(5), LONG);
  const page = await readPage("orders");
  expect(page.rows[3]).toEqual([
    value("integer", 4),
    value("text", 'Delta, "Co"'),
    value("text", "south"),
    value("integer", 7),
  ]);
  expect(page.rows[4][2]).toEqual(value("null"));

  await user.click(screen.getByRole("textbox", { name: "New id" }));
  await user.paste("6\tFox\tnorth\tmany\r\n7\tGolf\tnorth\t1\r\n");
  expect(await screen.findByRole("alert", {}, LONG)).toHaveTextContent(
    "Pasted row 1: qty requires an integer. Nothing was pasted.",
  );
  expect((await readPage("orders")).total).toBe(5);
});
