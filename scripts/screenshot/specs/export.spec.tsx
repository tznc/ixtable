import { screen, waitFor, within } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import { captureDocument } from "../capture";
import {
  dialogMock,
  LONG,
  openMode,
  refreshDatabase,
  renderNewDocument,
  seedSales,
  startFromTemplate,
  tempPath,
  type User,
} from "./fixtures";

const openExportMenu = async (user: User) => {
  await user.click(screen.getByRole("button", { name: "Export" }));
  return screen.findByRole("menu");
};

it("exports a filtered, sorted table from Data mode", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await refreshDatabase();
  await user.click(await screen.findByRole("button", { name: /^customers\b/ }, LONG));
  await screen.findByRole("textbox", { name: "name, row 1" }, LONG);
  const sortByName = () => screen.getByRole("button", { name: /^name/ });
  await user.click(sortByName());
  await user.click(sortByName());
  await waitFor(() => expect(sortByName()).toHaveTextContent("↓"), LONG);

  const menu = await openExportMenu(user);
  expect(
    within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual(["CSV", "Excel (.xlsx)", "JSON"]);
  await captureDocument(document, {
    name: "export-01-data-menu",
    expectations: [
      "The customers grid is sorted by name descending (↓ on the name header).",
      "An Export button sits in the grid toolbar next to Refresh, with its menu open below it.",
      "The menu lists CSV, Excel (.xlsx) and JSON.",
    ],
  });

  const path = tempPath("customers.csv");
  dialogMock.save.mockResolvedValueOnce(path);
  await user.click(within(menu).getByRole("menuitem", { name: "CSV" }));
  const status = await screen.findByText(/^Exported \d+ rows?$/, {}, LONG);
  const lines = readFileSync(path, "utf8").replace(/^\uFEFF/, "").trim().split("\r\n");
  expect(lines[0]).toBe("id,name,city");
  expect(status).toHaveTextContent(`Exported ${lines.length - 1} rows`);
  await captureDocument(document, {
    name: "export-02-data-done",
    expectations: [
      'The menu has closed and the toolbar shows a confirmation such as "Exported 4 rows".',
      "The grid itself is unchanged.",
    ],
  });
});

it("exports query results from Query mode", async () => {
  const user = await renderNewDocument();
  await seedSales();
  await openMode(user, "Query");
  await screen.findByRole("heading", { name: "Query" }, LONG);
  await user.click(screen.getByRole("button", { name: "New query" }));
  await user.click(screen.getByRole("tab", { name: "SQL" }));
  await user.click(await screen.findByRole("textbox", { name: "SQL editor" }));
  await user.paste("SELECT name, city FROM customers ORDER BY name");
  await user.click(screen.getByRole("button", { name: "Run" }));
  await screen.findByRole("region", { name: "Results" }, LONG);
  await screen.findByRole("button", { name: "Export" }, LONG);
  await openExportMenu(user);
  await captureDocument(document, {
    name: "export-03-query-menu",
    expectations: [
      "The SQL query has run and its results are listed.",
      "An Export button appears with the results, its CSV / Excel / JSON menu open.",
    ],
  });
  dialogMock.save.mockResolvedValueOnce(tempPath("customers.xlsx"));
  await user.click(screen.getByRole("menuitem", { name: "Excel (.xlsx)" }));
  await screen.findByText(/^Exported \d+ rows?$/, {}, LONG);
});

it("exports a runtime list form", async () => {
  const user = await startFromTemplate("Inventory");
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getAllByRole("button")[1]);
  const page = screen.getByRole("region", { name: "Application page" });
  await within(page).findAllByRole("row", { name: /^Open / }, LONG);
  const button = await within(page).findByRole("button", { name: "Export" }, LONG);
  await waitFor(() => expect(button).toBeEnabled(), LONG);
  dialogMock.save.mockResolvedValueOnce(tempPath("inventory.json"));
  await openExportMenu(user);
  await user.click(screen.getByRole("menuitem", { name: "JSON" }));
  await within(page).findByText(/^Exported \d+ rows?$/, {}, LONG);
  const rows = JSON.parse(readFileSync(tempPath("inventory.json"), "utf8"));
  expect(rows.length).toBeGreaterThan(0);
  await captureDocument(document, {
    name: "export-04-runtime-list",
    expectations: [
      "The Inventory list page shows its records with the pager below.",
      'The pager row has an Export button and an "Exported N rows" confirmation.',
    ],
  });
});
