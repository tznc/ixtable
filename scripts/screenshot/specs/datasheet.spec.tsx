import { screen, waitFor, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { captureDocument } from "../capture";
import { createTable, insert, LONG, refreshDatabase, renderNewDocument } from "./fixtures";

const ORDERS: Array<[number, string, string, number, number, string]> = [
  [1, "Acme Ltd", "North", 4, 120.5, "Open"],
  [2, "Bolt Ltd", "South", 2, 80, "Shipped"],
  [3, "Core Inc", "North", 10, 640, "Open"],
  [4, "Delta Ltd", "East", 1, 15.25, "Cancelled"],
  [5, "Echo Inc", "South", 7, 210, "Open"],
  [6, "Fox Ltd", "North", 3, 99.99, "Shipped"],
];

const names = () =>
  screen
    .getAllByRole("textbox", { name: /^customer, row \d+$/ })
    .map((input) => (input as HTMLInputElement).value);

it("uses the datasheet tools on a table", async () => {
  const user = await renderNewDocument();
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "customer", declaredType: "TEXT" },
    { name: "region", declaredType: "TEXT" },
    { name: "qty", declaredType: "INTEGER" },
    { name: "amount", declaredType: "REAL" },
    { name: "status", declaredType: "TEXT" },
  ]);
  for (const [id, customer, region, qty, amount, status] of ORDERS)
    await insert("orders", { id, customer, region, qty, amount, status });
  await refreshDatabase();
  await user.click(await screen.findByRole("button", { name: /^orders\b/ }, LONG));
  await screen.findByRole("textbox", { name: "status, row 6" }, LONG);

  await user.click(screen.getByRole("button", { name: /^region/ }));
  await user.keyboard("{Shift>}");
  await user.click(screen.getByRole("button", { name: /^qty/ }));
  await user.keyboard("{/Shift}");
  await waitFor(() => expect(names()[0]).toBe("Delta Ltd"), LONG);
  expect(names().slice(1, 4)).toEqual(["Fox Ltd", "Acme Ltd", "Core Inc"]);
  await captureDocument(document, {
    name: "datasheet-01-toolbar-and-multi-sort",
    expectations: [
      "A Datasheet tools bar above the grid shows Filter by selection, Filter excluding selection, Find and Totals.",
      "The region and qty headers show sort arrows with small 1 and 2 markers, and rows are ordered by region then qty.",
      "Each column header has a small options (⋮) button next to its name.",
    ],
  });

  await user.click(screen.getByRole("textbox", { name: "region, row 2" }));
  await user.click(screen.getByRole("button", { name: "Filter by selection" }));
  await waitFor(() => expect(names()).toEqual(["Fox Ltd", "Acme Ltd", "Core Inc"]), LONG);
  expect(screen.getByRole("list", { name: "Active filters" })).toHaveTextContent("region = North");
  await captureDocument(document, {
    name: "datasheet-02-filter-by-selection",
    expectations: [
      "A green 'region = North' filter chip with a remove (×) button and a 'Clear filters' button sits at the right of the tools bar.",
      "Only the three North rows are listed and the record count reads 3 records.",
    ],
  });
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  await waitFor(() => expect(names()).toHaveLength(6), LONG);

  await user.click(screen.getByRole("button", { name: "Find" }));
  const panel = screen.getByRole("dialog", { name: "Find and replace" });
  await user.type(within(panel).getByRole("textbox", { name: "Find" }), "ltd");
  await user.type(within(panel).getByRole("textbox", { name: "Replace with" }), "LLC");
  await user.click(within(panel).getByRole("button", { name: "Find next" }));
  await waitFor(
    () => expect(screen.getByRole("textbox", { name: "customer, row 1" })).toHaveFocus(),
    LONG,
  );
  await captureDocument(document, {
    name: "datasheet-03-find",
    expand: ".data-grid",
    expectations: [
      "A Find and replace panel under the tools bar shows Find 'ltd', Replace with 'LLC', Look in, Whole field, Match case, and Find next, Replace and Replace all buttons.",
      "The first matching cell (customer, row 1, 'Delta Ltd') is focused with a green focus outline.",
    ],
  });
  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  await user.click(within(panel).getByRole("button", { name: "Replace all" }));
  expect(await within(panel).findByRole("status", {}, LONG)).toHaveTextContent(
    "Replaced 4 matches in 4 records.",
  );
  await waitFor(() => expect(names()).toContain("Acme LLC"), LONG);
  confirm.mockRestore();
  await captureDocument(document, {
    name: "datasheet-04-replace-all",
    expand: ".data-grid",
    expectations: [
      "The panel reports 'Replaced 4 matches in 4 records.'",
      "Every customer that ended in 'Ltd' now ends in 'LLC'; 'Core Inc' and 'Echo Inc' are unchanged.",
    ],
  });
  await user.click(within(panel).getByRole("button", { name: "Close find and replace" }));

  await user.click(screen.getByRole("button", { name: "Totals" }));
  await user.selectOptions(
    await screen.findByRole("combobox", { name: "Total for qty" }, LONG),
    "sum",
  );
  await user.selectOptions(screen.getByRole("combobox", { name: "Total for amount" }), "avg");
  const totalCell = (column: string) =>
    within(screen.getByRole("combobox", { name: `Total for ${column}` }).closest("td")!);
  await waitFor(() => expect(totalCell("qty").getByRole("status")).toHaveTextContent("27"), LONG);
  await waitFor(
    () => expect(totalCell("amount").getByRole("status")).toHaveTextContent("194.29"),
    LONG,
  );
  await captureDocument(document, {
    name: "datasheet-05-totals-row",
    expand: ".data-grid",
    expectations: [
      "A grey Total row sits under the last record with a picker in every column.",
      "qty shows Sum 27 and amount shows Average 194.29; other columns read None.",
      "The Totals button in the tools bar is highlighted as pressed.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Column options for status" }));
  await user.click(await screen.findByRole("menuitem", { name: "Hide column" }));
  await waitFor(
    () => expect(screen.queryByRole("textbox", { name: "status, row 1" })).toBeNull(),
    LONG,
  );
  await user.click(screen.getByRole("button", { name: "Column options for customer" }));
  await user.click(await screen.findByRole("menuitem", { name: "Freeze through this column" }));
  const header = (column: string) =>
    screen.getByRole("button", { name: `Column options for ${column}` }).closest("th")!;
  await waitFor(() => expect(header("customer")).toHaveClass("frozen", "frozen-edge"), LONG);
  expect(header("id")).toHaveClass("frozen");
  expect(header("region")).not.toHaveClass("frozen");
  await captureDocument(document, {
    name: "datasheet-06-hidden-and-frozen",
    expand: ".data-grid",
    expectations: [
      "The status column is gone and the tools bar shows a '1 hidden' button and an 'Unfreeze columns' button.",
      "The id and customer columns are frozen, with a darker divider line after customer.",
    ],
  });

  await user.click(screen.getByRole("textbox", { name: "New id" }));
  await user.paste("7\tGolf LLC\tWest\t5\t50\r\n8\tHotel Inc\tWest\t2\t20.5\r\n");
  expect(await screen.findByText("Added 2 records.", {}, LONG)).toHaveAttribute("role", "status");
  await screen.findByRole("textbox", { name: "customer, row 8" }, LONG);
  await waitFor(() => expect(totalCell("qty").getByRole("status")).toHaveTextContent("34"), LONG);
  await captureDocument(document, {
    name: "datasheet-07-paste-from-spreadsheet",
    expand: ".data-grid",
    expectations: [
      "A notice under the tools bar reads 'Added 2 records.'",
      "Rows 7 and 8 (Golf LLC and Hotel Inc, region West) appear at the end of the grid.",
      "The totals row updates to qty Sum 34.",
    ],
  });
});
