import { screen, waitFor, within } from "@testing-library/react";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";
import { createTable, insertRow, renderNewDocument, value } from "./helpers";
import { dialogMock } from "./setup";

const LONG = { timeout: 20_000 };

async function seedPeople() {
  const user = await renderNewDocument();
  await createTable("people", [
    { name: "id", declaredType: "INTEGER", nullable: false, primaryKeyPosition: 1 },
    { name: "name", declaredType: "TEXT" },
  ]);
  for (const [id, name] of [
    [1, "ACME"],
    [2, "Bolt"],
    [3, "Core"],
  ] as const)
    await insertRow("people", [
      { column: "id", value: value("integer", id) },
      { column: "name", value: value("text", name) },
    ]);
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByRole("textbox", { name: "name, row 3" }, LONG);
  return user;
}

const chooseFormat = async (user: Awaited<ReturnType<typeof seedPeople>>, item: string) => {
  await user.click(screen.getByRole("button", { name: "Export" }));
  const menu = await screen.findByRole("menu");
  await user.click(within(menu).getByRole("menuitem", { name: item }));
};

it("exports a sorted table to CSV from Data mode and reports the row count", async () => {
  const user = await seedPeople();
  const sortByName = () => screen.getByRole("button", { name: /^name/ });
  await user.click(sortByName());
  await user.click(sortByName());
  await waitFor(() => expect(sortByName()).toHaveTextContent("↓"), LONG);

  const path = join(process.env.IXTABLE_STATE_DIR!, "people.csv");
  dialogMock.save.mockResolvedValueOnce(path);
  await chooseFormat(user, "CSV");
  expect(await screen.findByRole("status", {}, LONG)).toHaveTextContent("Exported 3 rows");
  const text = readFileSync(path, "utf8");
  // A BOM so Excel reads UTF-8, then CRLF rows.
  expect(text).toBe("\uFEFFid,name\r\n3,Core\r\n2,Bolt\r\n1,ACME\r\n");
});

it("does nothing when the save dialog is cancelled", async () => {
  const user = await seedPeople();
  dialogMock.save.mockResolvedValueOnce(null);
  await chooseFormat(user, "JSON");
  await waitFor(() => expect(dialogMock.save).toHaveBeenCalled());
  expect(screen.queryByText(/Exported/)).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
});

it("exports a saved query to JSON from Query mode", async () => {
  const user = await seedPeople();
  await user.click(screen.getByRole("button", { name: "Query" }));
  await screen.findByRole("heading", { name: "Query" }, LONG);
  await user.click(screen.getByRole("button", { name: "New query" }));
  await user.click(screen.getByRole("tab", { name: "SQL" }));
  const editor = await screen.findByRole("textbox", { name: "SQL editor" });
  await user.click(editor);
  await user.paste("SELECT name FROM people ORDER BY name DESC");
  const name = screen.getByRole("textbox", { name: "Query name" });
  await user.clear(name);
  await user.type(name, "People names");
  await user.click(screen.getByRole("button", { name: "Save query" }));
  await screen.findByRole("button", { name: "People names" }, LONG);

  // Nothing to export before a run.
  expect(screen.queryByRole("button", { name: "Export" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Run" }));
  await screen.findByRole("region", { name: "Results" }, LONG);
  await screen.findByRole("button", { name: "Export" }, LONG);

  const path = join(process.env.IXTABLE_STATE_DIR!, "names.json");
  dialogMock.save.mockResolvedValueOnce(path);
  await chooseFormat(user, "JSON");
  expect(await screen.findByText("Exported 3 rows", {}, LONG)).toBeInTheDocument();
  expect(existsSync(path)).toBe(true);
  const text = readFileSync(path, "utf8");
  expect(() => JSON.parse(text)).not.toThrow();
  expect(text.indexOf("Core")).toBeLessThan(text.indexOf("Bolt"));
  expect(text.indexOf("Bolt")).toBeLessThan(text.indexOf("ACME"));
});
