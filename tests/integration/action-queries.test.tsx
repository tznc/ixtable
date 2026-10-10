import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { runActionQuery } from "../../src/lib/records";
import { createTable, insertRow, readPage, renderNewDocument, value } from "./helpers";

const LONG = { timeout: 20_000 };
type User = Awaited<ReturnType<typeof renderNewDocument>>;

async function orders() {
  await createTable("orders", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "total", declaredType: "REAL" },
    { name: "due", declaredType: "DATE" },
    { name: "status", declaredType: "TEXT" },
  ]);
  for (const [id, total, due] of [
    [1, 10, "2024-01-31"],
    [2, 20, "2024-02-29"],
    [3, 30, "2024-03-31"],
  ] as const)
    await insertRow("orders", [
      { column: "id", value: value("integer", id) },
      { column: "total", value: value("real", total) },
      { column: "due", value: { type: "date", value: due } },
      { column: "status", value: value("text", "open") },
    ]);
}

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

async function authorUpdate(user: User) {
  await user.click(screen.getByRole("button", { name: "Query" }));
  await user.click(await screen.findByRole("button", { name: "Create a query" }, LONG));
  const name = screen.getByRole("textbox", { name: "Query name" });
  await user.clear(name);
  await user.type(name, "Extend due dates");
  await user.selectOptions(screen.getByRole("combobox", { name: "Query type" }), "update");
  await user.selectOptions(screen.getByRole("combobox", { name: "Target table" }), "orders");
  const sql = screen.getByRole("textbox", { name: "SQL editor" });
  expect(sql).toHaveValue("UPDATE orders\nSET ... \nWHERE ...");
  await user.clear(sql);
  await user.click(sql);
  await user.paste("UPDATE orders SET due = due + 7, status = 'extended' WHERE total >= 20");
  await user.click(screen.getByRole("button", { name: "Save query" }));
  await waitFor(
    () => expect(screen.getByRole("button", { name: "Preview changes" })).toBeEnabled(),
    LONG,
  );
}

it("authors, previews and runs an action query in Query mode", async () => {
  const user = await renderNewDocument();
  await orders();
  await authorUpdate(user);
  expect(await screen.findByText("Action query")).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Preview changes" }));
  expect(
    await screen.findByText("Would update 2 rows in orders. Nothing was changed.", {}, LONG),
  ).toBeInTheDocument();
  expect((await readPage("orders")).rows.map((r) => r[3])).toEqual([
    value("text", "open"),
    value("text", "open"),
    value("text", "open"),
  ]);

  const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
  await user.click(screen.getByRole("button", { name: "Run" }));
  expect(await screen.findByText("Updated 2 rows in orders.", {}, LONG)).toBeInTheDocument();
  expect(confirm).toHaveBeenCalledWith(
    "Run Extend due dates? It changes rows in orders and cannot be undone.",
  );
  confirm.mockRestore();
  const page = await readPage("orders");
  expect(page.rows.map((r) => [r[2], r[3]])).toEqual([
    [{ type: "date", value: "2024-01-31" }, value("text", "open")],
    [{ type: "date", value: "2024-03-07" }, value("text", "extended")],
    [{ type: "date", value: "2024-04-07" }, value("text", "extended")],
  ]);
});

it("refuses SQL that writes another table, and action queries as read sources", async () => {
  const user = await renderNewDocument();
  await orders();
  await user.click(screen.getByRole("button", { name: "Query" }));
  await user.click(await screen.findByRole("button", { name: "Create a query" }, LONG));
  await user.selectOptions(screen.getByRole("combobox", { name: "Query type" }), "delete");
  await user.selectOptions(screen.getByRole("combobox", { name: "Target table" }), "orders");
  const sql = screen.getByRole("textbox", { name: "SQL editor" });
  await user.clear(sql);
  await user.click(sql);
  await user.paste("DROP TABLE orders");
  await user.click(screen.getByRole("button", { name: "Save query" }));
  expect(await screen.findByRole("alert", {}, LONG)).toHaveTextContent(/cannot use DROP/);
  await editConfig(user, () => ({
    savedQueries: [
      {
        id: "q-del",
        name: "Purge",
        sql: "DELETE FROM orders",
        parameters: [],
        action: { kind: "delete", table: "orders" },
      },
    ],
  }));
  const error = await invoke("run_saved_query", {
    windowLabel: "main",
    id: "q-del",
    params: [],
    limit: null,
    runId: null,
  }).catch((e) => e as { message: string });
  expect((error as { message: string }).message).toMatch(/is an action query/);
  expect((await readPage("orders")).total).toBe(3);
});

it("fires the target table's triggers once per changed row", async () => {
  const user = await renderNewDocument();
  await orders();
  await createTable("audit", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "message", declaredType: "TEXT" },
  ]);
  await editConfig(user, () => ({
    savedQueries: [
      {
        id: "q-close",
        name: "Close big orders",
        sql: "UPDATE orders SET status = 'closed' WHERE total > $min",
        parameters: [{ name: "min", logicalType: "number" }],
        action: { kind: "update", table: "orders" },
      },
      {
        id: "q-copy",
        name: "Copy closed",
        sql: "INSERT INTO orders (id, total, status) SELECT id + 100, total, 'copy' FROM orders WHERE status = 'closed'",
        parameters: [],
        action: { kind: "insert", table: "orders" },
      },
    ],
    actions: [
      {
        id: "a-audit",
        name: "Audit",
        onError: "stop",
        steps: [
          {
            id: "s1",
            kind: "createRecord",
            table: "audit",
            values: {
              message:
                "'order ' & record.id & ': ' & coalesce(old.status, 'new') & ' → ' & record.status",
            },
          },
        ],
      },
    ],
    triggers: ["updated", "created"].map((event) => ({
      id: `t-${event}`,
      name: `Audit ${event}`,
      table: "orders",
      event,
      actionId: "a-audit",
      mode: "sync",
      enabled: true,
      maxAttempts: 1,
      backoffMs: 0,
    })),
  }));
  const closed = await runActionQuery("q-close", [{ column: "min", value: value("integer", 15) }]);
  expect([closed.changed, closed.updated.length]).toEqual([2, 2]);
  const copied = await runActionQuery("q-copy", []);
  expect(copied.created.length).toBe(2);
  const messages = (await readPage("audit")).rows.map((r) => String(r[1].value)).sort();
  expect(messages).toEqual([
    "order 102: new → copy",
    "order 103: new → copy",
    "order 2: open → closed",
    "order 3: open → closed",
  ]);
  const dry = await runActionQuery("q-close", [{ column: "min", value: value("integer", 0) }], {
    dryRun: true,
  });
  expect(dry.changed).toBe(5);
  expect((await readPage("audit")).total).toBe(4);
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  expect(within(document.body).queryByText(/Trigger .* failed/)).toBeNull();
});
