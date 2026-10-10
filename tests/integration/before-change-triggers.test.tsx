import { screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { deleteRecord, insertRecord, runActionQuery } from "../../src/lib/records";
import { createTable, insertRow, readPage, renderNewDocument, value } from "./helpers";

const LONG = { timeout: 20_000 };
type User = Awaited<ReturnType<typeof renderNewDocument>>;
const text = (v: string) => value("text", v);
const today = () => new Date().toISOString().slice(0, 10);

async function setup() {
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "total", declaredType: "REAL" },
    { name: "due", declaredType: "DATE" },
    { name: "status", declaredType: "TEXT" },
  ]);
  await createTable("audit", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "message", declaredType: "TEXT" },
  ]);
  for (const [id, total] of [
    [1, 10],
    [2, 20],
    [3, 30],
  ] as const)
    await insertRow("orders", [
      { column: "id", value: value("integer", id) },
      { column: "total", value: value("real", total) },
      { column: "due", value: { type: "date", value: "2024-01-31" } },
      { column: "status", value: text("open") },
    ]);
}

// Applies config changes the way the other integration tests do, then reloads the Entities tab.
async function editConfig(user: User, extra: (config: Record<string, unknown>) => object) {
  await user.click(screen.getByRole("button", { name: "Settings" }));
  await user.click(await screen.findByRole("tab", { name: "Entities" }, LONG));
  await screen.findByRole("combobox", { name: "Concurrency policy for orders" }, LONG);
  const config = await invoke<Record<string, unknown>>("read_document_config", {
    windowLabel: "main",
  });
  await invoke("update_document_config", {
    windowLabel: "main",
    config: { ...config, ...extra(config) },
  });
  await user.click(screen.getByRole("tab", { name: "Datasource" }));
  await user.click(screen.getByRole("tab", { name: "Entities" }));
  await screen.findByRole("heading", { name: "Entities" }, LONG);
}

const action = (id: string, steps: Array<Record<string, unknown>>) => ({
  id,
  name: id,
  onError: "stop",
  steps: steps.map((s, i) => ({ id: `${id}-${i}`, ...s })),
});
const trigger = (id: string, event: string, actionId: string, extra: object = {}) => ({
  id,
  name: id,
  table: "orders",
  event,
  actionId,
  mode: "sync",
  enabled: true,
  maxAttempts: 1,
  backoffMs: 0,
  ...extra,
});
const column = async (table: string, name: string) => {
  const page = await readPage(table);
  const index = page.columns.findIndex((c) => c.name === name);
  return page.rows.map((r) => r[index]);
};
const auditMessages = async () => (await column("audit", "message")).map((v) => v.value).sort();

it("stores a field set by a before-change trigger on insert and rejects without a row", async () => {
  const user = await renderNewDocument();
  await setup();
  await editConfig(user, () => ({
    actions: [
      action("a-stamp", [
        { kind: "setField", field: "status", value: "upper(record.status)" },
        { kind: "fail", message: "'Total must be positive'", when: "record.total <= 0" },
      ]),
    ],
    triggers: [trigger("t-before", "beforeChange", "a-stamp")],
  }));
  await insertRecord("orders", [
    { column: "id", value: value("integer", 4) },
    { column: "total", value: value("real", 5) },
    { column: "status", value: text("new") },
  ]);
  expect((await column("orders", "status"))[3]).toEqual(text("NEW"));

  await expect(
    insertRecord("orders", [
      { column: "id", value: value("integer", 5) },
      { column: "total", value: value("real", -1) },
      { column: "status", value: text("bad") },
    ]),
  ).rejects.toThrow("Not saved: Total must be positive");
  expect((await readPage("orders")).total).toBe(4);
});

it("writes an audit row from a deleted trigger with the deleted row's values", async () => {
  const user = await renderNewDocument();
  await setup();
  await editConfig(user, () => ({
    actions: [
      action("a-audit", [
        {
          kind: "createRecord",
          table: "audit",
          values: { message: "'deleted ' & record.id & ' ' & record.status & ' ' & old.total" },
        },
      ]),
    ],
    triggers: [trigger("t-del", "deleted", "a-audit")],
  }));
  await deleteRecord("orders", [value("integer", 2)], {
    expected: [
      { column: "id", value: value("integer", 2) },
      { column: "total", value: value("real", 20) },
      { column: "due", value: { type: "date", value: "2024-01-31" } },
      { column: "status", value: text("open") },
    ],
  });
  expect((await readPage("orders")).total).toBe(2);
  expect(await auditMessages()).toEqual(["deleted 2 open 20"]);
});

it("applies before-change fields to every row of an update action query, or rejects it", async () => {
  const user = await renderNewDocument();
  await setup();
  await editConfig(user, () => ({
    savedQueries: [
      {
        id: "q-close",
        name: "Close orders",
        sql: "UPDATE orders SET status = 'closed' WHERE total < $max",
        parameters: [{ name: "max", logicalType: "number" }],
        action: { kind: "update", table: "orders" },
      },
    ],
    actions: [
      action("a-date", [
        { kind: "setField", field: "due", value: "today()" },
        { kind: "fail", message: "'Orders over 25 stay open'", when: "record.total > 25" },
      ]),
    ],
    triggers: [trigger("t-before", "beforeChange", "a-date")],
  }));
  const before = await readPage("orders");

  // Order 3 is rejected, so the whole table stays as it was.
  await expect(
    runActionQuery("q-close", [{ column: "max", value: value("integer", 100) }]),
  ).rejects.toThrow("Not saved: Orders over 25 stay open");
  expect((await readPage("orders")).rows).toEqual(before.rows);

  const run = await runActionQuery("q-close", [{ column: "max", value: value("integer", 25) }]);
  expect([run.dryRun, run.changed]).toEqual([false, 2]);
  const page = await readPage("orders");
  expect(page.rows.map((r) => [r[2], r[3]])).toEqual([
    [{ type: "date", value: today() }, text("closed")],
    [{ type: "date", value: today() }, text("closed")],
    [{ type: "date", value: "2024-01-31" }, text("open")],
  ]);
}, 60_000);

it("applies a before-change field to rows an insert action query creates", async () => {
  const user = await renderNewDocument();
  await setup();
  await editConfig(user, () => ({
    savedQueries: [
      {
        id: "q-copy",
        name: "Copy orders",
        sql: "INSERT INTO orders (id, total, status) SELECT id + 100, total, 'copy' FROM orders",
        parameters: [],
        action: { kind: "insert", table: "orders" },
      },
    ],
    actions: [
      action("a-set", [{ kind: "setField", field: "status", value: "record.status & '!'" }]),
    ],
    triggers: [trigger("t-before", "beforeChange", "a-set")],
  }));
  const run = await runActionQuery("q-copy", []);
  // Only created/updated/deleted triggers make Rust list the created rows.
  expect(run.changed).toBe(3);
  const page = await readPage("orders");
  expect((await column("orders", "status")).slice(3)).toEqual([
    text("copy!"),
    text("copy!"),
    text("copy!"),
  ]);
  expect(page.total).toBe(6);
}, 60_000);

it("fires deleted triggers once per row of a delete action query", async () => {
  const user = await renderNewDocument();
  await setup();
  await editConfig(user, () => ({
    savedQueries: [
      {
        id: "q-purge",
        name: "Purge small orders",
        sql: "DELETE FROM orders WHERE total <= 20",
        parameters: [],
        action: { kind: "delete", table: "orders" },
      },
    ],
    actions: [
      action("a-audit", [
        {
          kind: "createRecord",
          table: "audit",
          values: { message: "'purged ' & record.id & ' total ' & old.total" },
        },
      ]),
    ],
    triggers: [trigger("t-del", "deleted", "a-audit")],
  }));
  const run = await runActionQuery("q-purge", []);
  expect(run.deleted?.length).toBe(2);
  expect((await readPage("orders")).total).toBe(1);
  expect(await auditMessages()).toEqual(["purged 1 total 10", "purged 2 total 20"]);
}, 60_000);
