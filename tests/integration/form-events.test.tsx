import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { createTable, insertRow, readPage, renderNewDocument, value } from "./helpers";

const LONG = { timeout: 20_000 };
const text = (v: string) => value("text", v);

const actions = [
  {
    id: "greet",
    name: "Greet",
    onError: "stop",
    steps: [{ id: "g1", kind: "message", text: "'Hello ' & app.user.name" }],
  },
  {
    id: "visit",
    name: "Log visit",
    onError: "stop",
    steps: [
      {
        id: "v1",
        kind: "createRecord",
        table: "audit",
        values: { message: "'current ' & record.id" },
      },
    ],
  },
  {
    id: "guard",
    name: "Guard status",
    onError: "stop",
    steps: [
      { id: "b1", kind: "fail", when: "record.status = 'bad'", message: "'Status cannot be bad'" },
    ],
  },
  {
    id: "after",
    name: "Log save",
    onError: "stop",
    steps: [
      {
        id: "a1",
        kind: "createRecord",
        table: "audit",
        values: { message: "'saved ' & record.status" },
      },
    ],
  },
];

it("runs on load, on current, before update (veto) and after update actions on a form", async () => {
  const user = await renderNewDocument();
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "status", declaredType: "TEXT" },
  ]);
  await createTable("audit", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "message", declaredType: "TEXT" },
  ]);
  await insertRow("orders", [
    { column: "id", value: value("integer", 1) },
    { column: "status", value: text("open") },
  ]);
  // Actions go in before the Studio edits the design, so the store does not overwrite them.
  await user.click(screen.getByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("tab", { name: "Entities" }, LONG));
  await screen.findByRole("combobox", { name: "Concurrency policy for orders" }, LONG);
  const config = await invoke<Record<string, unknown>>("read_document_config", {
    windowLabel: "main",
  });
  await invoke("update_document_config", { windowLabel: "main", config: { ...config, actions } });
  await user.click(screen.getByRole("tab", { name: "Datasource" }));
  await user.click(screen.getByRole("tab", { name: "Entities" }));
  await screen.findByRole("heading", { name: "Entities" }, LONG);

  await user.click(screen.getByRole("button", { name: "Design" }));
  await screen.findByRole("region", { name: "Form builder" }, LONG);
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Table to generate from" }),
    "orders",
  );
  await user.click(screen.getByRole("button", { name: "Generate form from table" }));
  await within(screen.getByRole("region", { name: "Forms" })).findByRole(
    "button",
    { name: "Orders list" },
    LONG,
  );
  const properties = screen.getByRole("complementary", { name: "Properties" });
  for (const [event, action] of [
    ["On load", "Greet"],
    ["On current", "Log visit"],
    ["Before update", "Guard status"],
    ["After update", "Log save"],
  ])
    await user.selectOptions(
      await within(properties).findByRole("combobox", { name: event }, LONG),
      action,
    );
  await waitFor(async () => {
    const saved = await invoke<{
      design: { forms: Array<{ name: string; events?: Record<string, string> }> };
    }>("read_document_config", { windowLabel: "main" });
    expect(saved.design.forms.find((f) => f.name === "Orders")?.events).toEqual({
      onLoad: "greet",
      onCurrent: "visit",
      beforeUpdate: "guard",
      afterUpdate: "after",
    });
  }, LONG);

  await user.click(screen.getByRole("button", { name: "Runtime" }));
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Orders" }));
  await user.click(await within(page).findByRole("row", { name: /Open/ }, LONG));
  const detail = await screen.findByRole("form", { name: "Orders" }, LONG);
  await within(detail).findByText("Hello Developer", {}, LONG);
  await waitFor(
    async () =>
      expect((await readPage("audit")).rows.map((r) => r[1])).toEqual([text("current 1")]),
    LONG,
  );

  // Entering edit mode stays on the same record, so on current does not run again.
  await user.click(within(detail).getByRole("button", { name: "Edit" }));
  const edit = await screen.findByRole("form", { name: "Edit Orders" }, LONG);
  const status = await within(edit).findByDisplayValue("open", {}, LONG);
  await user.clear(status);
  await user.type(status, "bad");
  await user.click(within(edit).getByRole("button", { name: "Save" }));
  await within(edit).findByText("Status cannot be bad", {}, LONG);
  expect((await readPage("orders")).rows[0][1]).toEqual(text("open"));

  await user.clear(status);
  await user.type(status, "paid");
  await user.click(within(edit).getByRole("button", { name: "Save" }));
  const shown = await screen.findByRole("form", { name: "Orders" }, LONG);
  await within(shown).findByDisplayValue("paid", {}, LONG);
  await waitFor(
    async () =>
      expect((await readPage("audit")).rows.map((r) => r[1])).toEqual([
        text("current 1"),
        text("saved paid"),
      ]),
    LONG,
  );
});
