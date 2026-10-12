import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { captureDocument } from "../capture";
import {
  createTable,
  insert,
  LONG,
  openMode,
  openSettingsTab,
  renderNewDocument,
} from "./fixtures";

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
        values: { message: "'viewed order ' & record.id" },
      },
    ],
  },
  {
    id: "guard",
    name: "Guard status",
    onError: "stop",
    steps: [
      {
        id: "b1",
        kind: "fail",
        when: "record.status = 'bad'",
        message: "'Status cannot be bad'",
      },
    ],
  },
  {
    id: "after",
    name: "Log save",
    onError: "stop",
    steps: [{ id: "a1", kind: "message", text: "'Order saved as ' & record.status" }],
  },
];

it("binds form events in the Studio and runs them in the Runtime", async () => {
  const user = await renderNewDocument();
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "status", declaredType: "TEXT" },
  ]);
  await createTable("audit", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "message", declaredType: "TEXT" },
  ]);
  await insert("orders", { id: 1, status: "open" });
  // Actions go in before the Studio edits the design, so the store does not overwrite them.
  await openSettingsTab(user, "Entities");
  await screen.findByRole("combobox", { name: "Concurrency policy for orders" }, LONG);
  const config = await invoke<Record<string, unknown>>("read_document_config", {
    windowLabel: "main",
  });
  await invoke("update_document_config", { windowLabel: "main", config: { ...config, actions } });
  await user.click(screen.getByRole("tab", { name: "Datasource" }));
  await user.click(screen.getByRole("tab", { name: "Entities" }));
  await screen.findByRole("heading", { name: "Entities" }, LONG);

  await openMode(user, "Design");
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
  const events = within(properties).getByRole("group", { name: "Events" });
  expect(within(events).getByRole("combobox", { name: "Before update" })).toHaveDisplayValue(
    "Guard status",
  );
  events.scrollIntoView();
  await captureDocument(document, {
    name: "form-events-01-designer",
    expand: ".studio-properties",
    expectations: [
      "The Orders form is selected in Design mode.",
      "The Properties panel ends with an Events section listing On load, On current, Before update and After update.",
      "The four pickers show Greet, Log visit, Guard status and Log save.",
    ],
  });

  await openMode(user, "Runtime");
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Orders" }));
  await user.click(await within(page).findByRole("row", { name: /Open/ }, LONG));
  const detail = await screen.findByRole("form", { name: "Orders" }, LONG);
  await within(detail).findByText("Hello Developer", {}, LONG);
  await within(detail).findByDisplayValue("open", {}, LONG);
  await captureDocument(document, {
    name: "form-events-02-on-load",
    expectations: [
      "The Orders record view shows order 1 with status open.",
      "A 'Hello Developer' notice from the on load action shows under the heading.",
    ],
  });

  await user.click(within(detail).getByRole("button", { name: "Edit" }));
  const edit = await screen.findByRole("form", { name: "Edit Orders" }, LONG);
  const status = await within(edit).findByDisplayValue("open", {}, LONG);
  await user.clear(status);
  await user.type(status, "bad");
  await user.click(within(edit).getByRole("button", { name: "Save" }));
  await within(edit).findByText("Status cannot be bad", {}, LONG);
  await captureDocument(document, {
    name: "form-events-03-before-update-veto",
    expectations: [
      "The Edit Orders form still shows status 'bad' with Cancel and Save buttons.",
      "An error 'Status cannot be bad' from the before update action shows; the record was not saved.",
    ],
  });

  await user.clear(status);
  await user.type(status, "paid");
  await user.click(within(edit).getByRole("button", { name: "Save" }));
  const shown = await screen.findByRole("form", { name: "Orders" }, LONG);
  await within(shown).findByDisplayValue("paid", {}, LONG);
  await within(shown).findByText("Order saved as paid", {}, LONG);
  await waitFor(async () => {
    const audit = await invoke<{ rows: unknown[] }>("read_table_page", {
      windowLabel: "main",
      table: "audit",
      offset: 0,
      limit: 10,
      sorts: [],
      filters: [],
    });
    expect(audit.rows).toHaveLength(1);
  }, LONG);
  await captureDocument(document, {
    name: "form-events-04-after-update",
    expectations: [
      "The Orders record view shows status paid after the save.",
      "An 'Order saved as paid' notice from the after update action shows under the heading.",
    ],
  });
});
