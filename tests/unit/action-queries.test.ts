import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentConfig } from "../../src/lib/types";
import { createFakeBackend } from "./automation-fakes";

const state = vi.hoisted(() => ({
  backend: null as null | { invoke: (c: string, a?: Record<string, unknown>) => Promise<unknown> },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => state.backend?.invoke(command, args),
}));

const { runActionQuery, registerRecordHook } = await import("../../src/lib/records");
const { installTriggers } = await import("../../src/automation/triggers");
const { runAction } = await import("../../src/automation/runner");
const { browserContext } = await import("../../src/automation/context");
const { describeRun } = await import("../../src/query/actionText");
type ActionDef = import("../../src/automation/types").ActionDef;
type ActionContext = import("../../src/automation/runner").ActionContext;

const int = (value: number) => ({ type: "integer" as const, value });
const text = (value: string) => ({ type: "text" as const, value });

let db: ReturnType<typeof createFakeBackend>;
let config: DocumentConfig;
let uninstall: () => void;

function markPaid(args: Record<string, unknown>) {
  const open = db.rows("orders").filter((r) => r.status === "open");
  const updated = open.map((r) => ({
    identity: [int(r.id as number)],
    old: [
      { column: "id", value: int(r.id as number) },
      { column: "status", value: text("open") },
    ],
  }));
  if (!args.dryRun) for (const r of open) r.status = "paid";
  return {
    changed: open.length,
    removed: 0,
    dryRun: !!args.dryRun,
    table: "orders",
    created: [],
    updated: args.dryRun ? [] : updated,
    createdGrant: null,
    updatedGrant: args.dryRun ? null : "grant-1",
  };
}

const audit: ActionDef = {
  id: "audit",
  name: "Audit",
  onError: "stop",
  steps: [
    {
      id: "s1",
      kind: "createRecord",
      table: "audit",
      values: { message: "'order ' & record.id & ': ' & old.status & ' → ' & record.status" },
    },
  ],
};

beforeEach(() => {
  db = createFakeBackend();
  state.backend = db;
  db.addTable(
    "orders",
    ["id", "status"],
    [
      { id: 1, status: "open" },
      { id: 2, status: "open" },
      { id: 3, status: "paid" },
    ],
  );
  db.addTable("audit", ["id", "message"]);
  db.on("run_action_query", markPaid);
  db.on("release_trigger_grant", () => null);
  config = {
    actions: [audit],
    triggers: [
      {
        id: "t1",
        name: "Audit orders",
        table: "orders",
        event: "updated",
        actionId: "audit",
        mode: "sync",
        enabled: true,
        maxAttempts: 1,
        backoffMs: 0,
      },
    ],
    savedQueries: [
      {
        id: "pay",
        name: "Mark paid",
        sql: "UPDATE orders SET status = 'paid' WHERE status = 'open'",
        action: { kind: "update", table: "orders" },
      },
    ],
    design: { forms: [] },
  } as unknown as DocumentConfig;
  uninstall = installTriggers({
    getConfig: () => config,
    context: (base) => browserContext(config, () => undefined, base),
  });
});
afterEach(() => uninstall());

describe("runActionQuery", () => {
  it("fires updated triggers once per changed row and releases the shared grant once", async () => {
    const run = await runActionQuery("pay", []);
    expect(run.changed).toBe(2);
    expect(db.rows("audit").map((r) => r.message)).toEqual([
      "order 1: open → paid",
      "order 2: open → paid",
    ]);
    const released = db.calls.filter((c) => c.command === "release_trigger_grant");
    expect(released.map((c) => c.args.grant)).toEqual(["grant-1"]);
  });

  it("tells every record hook once, and nothing on a dry run", async () => {
    const after = vi.fn();
    const unregister = registerRecordHook({ after });
    await runActionQuery("pay", [], { dryRun: true });
    expect(after).not.toHaveBeenCalled();
    expect(db.rows("orders").filter((r) => r.status === "paid")).toHaveLength(1);
    await runActionQuery("pay", []);
    const orders = after.mock.calls.filter(([write]) => write.table === "orders");
    expect(orders).toHaveLength(1);
    expect(orders[0][0].meta.bulk.updated).toHaveLength(2);
    unregister();
  });
});

describe("runQuery steps on action queries", () => {
  const ctx = (extra: Partial<ActionContext> = {}): ActionContext => ({
    config,
    app: {},
    navigate: () => undefined,
    setState: () => undefined,
    confirm: async () => true,
    notify: () => undefined,
    ...extra,
  });
  const step = { id: "q", kind: "runQuery" as const, queryId: "pay", params: {}, storeAs: "paid" };

  it("run the query and store the counts", async () => {
    const result = await runAction(
      { id: "a", name: "Pay all", onError: "stop", steps: [step] },
      ctx(),
    );
    expect(result.ok).toBe(true);
    expect(result.results.paid).toEqual({ changed: 2, removed: 0 });
  });

  it("are refused in rollback-mode actions and without the table permission", async () => {
    const rollback = await runAction(
      { id: "a", name: "Pay all", onError: "rollback", steps: [step] },
      ctx(),
    );
    expect(rollback.error).toMatch(/cannot run inside an action that rolls back/);
    const refused = await runAction(
      { id: "a", name: "Pay all", onError: "stop", steps: [step] },
      ctx({ authorize: (kind, _id, op) => !(kind === "table" && op === "update") }),
    );
    expect(refused.error).toMatch(/Not permitted/);
    expect(db.rows("orders").filter((r) => r.status === "open")).toHaveLength(2);
  });
});

it("describes runs in words", () => {
  const run = { changed: 2, removed: 0, dryRun: true, table: "orders" };
  expect(describeRun(run, "update")).toBe("Would update 2 rows in orders. Nothing was changed.");
  expect(describeRun({ ...run, dryRun: false, changed: 1 }, "delete")).toBe(
    "Deleted 1 row in orders.",
  );
  expect(describeRun({ ...run, dryRun: false, removed: 5 }, "replace")).toBe(
    "Inserted 2 rows in orders after removing 5 rows.",
  );
});
