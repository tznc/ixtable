import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DocumentConfig } from "../../src/lib/types";
import { createFakeBackend } from "./automation-fakes";

const state = vi.hoisted(() => ({
  backend: null as null | { invoke: (c: string, a?: Record<string, unknown>) => Promise<unknown> },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: Record<string, unknown>) => state.backend?.invoke(command, args),
}));

const { installTriggers, BeforeChangeRejected } = await import("../../src/automation/triggers");
const { insertRecord, updateRecord, deleteRecord, writeRecordBatch, runActionQuery } = await import(
  "../../src/lib/records"
);
const { browserContext } = await import("../../src/automation/context");
type Trigger = import("../../src/automation/types").Trigger;
type ActionDef = import("../../src/automation/types").ActionDef;
type Step = import("../../src/automation/types").Step;

let db: ReturnType<typeof createFakeBackend>;
let config: DocumentConfig;
let uninstall: () => void;

const text = (value: string) => ({ type: "text" as const, value });
const int = (value: number) => ({ type: "integer" as const, value });
const named = (column: string, value: string) => ({ column, value: text(value) });
const action = (id: string, steps: Step[]): ActionDef => ({ id, name: id, onError: "stop", steps });
const set = (id: string, field: string, value: string): Step => ({
  id,
  kind: "setField",
  field,
  value,
});
const trigger = (t: Partial<Trigger>): Trigger => ({
  id: "t1",
  name: "Check orders",
  table: "orders",
  event: "beforeChange",
  actionId: "stamp",
  mode: "sync",
  enabled: true,
  maxAttempts: 1,
  backoffMs: 0,
  ...t,
});
const auditAction = action("audit", [
  {
    id: "a1",
    kind: "createRecord",
    table: "audit",
    values: { message: "'deleted ' & record.id & ' ' & record.status & ' old=' & old.status" },
  },
]);

beforeEach(() => {
  db = createFakeBackend();
  state.backend = db;
  db.addTable("orders", ["id", "status", "note"], [{ id: 1, status: "open", note: null }]);
  db.addTable("audit", ["id", "message"]);
  config = {
    actions: [action("stamp", [set("s1", "note", "'stamped ' & record.status")]), auditAction],
    triggers: [],
    savedQueries: [],
    design: { forms: [] },
  } as unknown as DocumentConfig;
  uninstall = installTriggers({
    getConfig: () => config,
    context: (base) => browserContext(config, () => undefined, base),
  });
});
afterEach(() => uninstall());

describe("beforeChange", () => {
  it("sets a field on insert and the stored row has it", async () => {
    config.triggers = [trigger({})];
    await insertRecord("orders", [named("status", "new")]);
    expect(db.rows("orders")[1]).toMatchObject({ status: "new", note: "stamped new" });
    const sent = db.calls.find((c) => c.command === "insert_row" && c.args.table === "orders");
    expect(sent?.args.values).toContainEqual({ column: "note", value: text("stamped new") });
  });

  it("sets a field on update from the old row merged with the new values", async () => {
    config.actions = [
      action("stamp", [set("s1", "note", "old.status & '->' & record.status & ' #' & record.id")]),
    ];
    config.triggers = [trigger({})];
    await updateRecord("orders", [named("status", "closed")], [int(1)]);
    expect(db.rows("orders")[0]).toMatchObject({ status: "closed", note: "open->closed #1" });
  });

  it("gives inserts a null old row", async () => {
    config.actions = [action("stamp", [set("s1", "note", "coalesce(old.status, 'none')")])];
    config.triggers = [trigger({})];
    await insertRecord("orders", [named("status", "new")]);
    expect(db.rows("orders")[1].note).toBe("none");
  });

  it("rejects with the fail step's message and writes nothing", async () => {
    config.actions = [
      action("stamp", [{ id: "f", kind: "fail", message: "'Status is required'", when: "true" }]),
    ];
    config.triggers = [trigger({})];
    const error = await insertRecord("orders", [named("status", "x")]).catch((e) => e);
    expect(error).toBeInstanceOf(BeforeChangeRejected);
    expect(error.message).toBe("Not saved: Status is required");
    expect(db.rows("orders")).toHaveLength(1);
    expect(db.calls.some((c) => c.command === "insert_row")).toBe(false);
  });

  it("rejects an update with a failing condition-gated fail step and leaves the row", async () => {
    config.actions = [
      action("stamp", [
        {
          id: "f",
          kind: "fail",
          message: "'Closed orders are final'",
          when: "old.status = 'closed'",
        },
      ]),
    ];
    config.triggers = [trigger({})];
    db.rows("orders")[0].status = "closed";
    await expect(updateRecord("orders", [named("status", "open")], [int(1)])).rejects.toThrow(
      "Not saved: Closed orders are final",
    );
    expect(db.rows("orders")[0].status).toBe("closed");
    expect(db.calls.some((c) => c.command === "update_row")).toBe(false);
  });

  it("names the trigger when a step errors", async () => {
    config.actions = [action("stamp", [set("s1", "note", "1 +")])];
    config.triggers = [trigger({})];
    await expect(insertRecord("orders", [named("status", "x")])).rejects.toThrow(
      /^Not saved: trigger "Check orders" failed: /,
    );
    expect(db.rows("orders")).toHaveLength(1);
  });

  it("refuses steps that write, with a message naming the step", async () => {
    config.actions = [
      action("stamp", [
        { id: "c", kind: "createRecord", table: "audit", values: { message: "'x'" } },
      ]),
    ];
    config.triggers = [trigger({})];
    await expect(insertRecord("orders", [named("status", "x")])).rejects.toThrow(
      'Not saved: A before-change trigger cannot run "Create record" steps; use an after trigger',
    );
    expect(db.rows("orders")).toHaveLength(1);
    expect(db.rows("audit")).toHaveLength(0);
  });

  it("skips a trigger whose condition is false", async () => {
    config.triggers = [trigger({ condition: "record.status = 'open'" })];
    await insertRecord("orders", [named("status", "draft")]);
    await insertRecord("orders", [named("status", "open")]);
    expect(db.rows("orders").map((r) => r.note ?? null)).toEqual([null, null, "stamped open"]);
  });

  it("skips disabled triggers", async () => {
    config.triggers = [trigger({ enabled: false })];
    await insertRecord("orders", [named("status", "open")]);
    expect(db.rows("orders")[1].note ?? null).toBeNull();
  });

  it("chains triggers in order, each seeing the fields earlier ones set", async () => {
    config.actions = [
      action("first", [set("s1", "note", "'one'")]),
      action("second", [set("s2", "note", "record.note & '+two'")]),
    ];
    config.triggers = [
      trigger({ id: "t1", actionId: "first" }),
      trigger({ id: "t2", actionId: "second" }),
    ];
    await insertRecord("orders", [named("status", "x")]);
    expect(db.rows("orders")[1].note).toBe("one+two");
  });

  it("sees later steps' updated record within one action", async () => {
    config.actions = [
      action("stamp", [set("s1", "note", "'a'"), set("s2", "status", "record.note & 'b'")]),
    ];
    config.triggers = [trigger({})];
    await insertRecord("orders", [named("status", "x")]);
    expect(db.rows("orders")[1]).toMatchObject({ note: "a", status: "ab" });
  });

  it("stops the chain when an earlier trigger rejects", async () => {
    config.actions = [
      action("first", [{ id: "f", kind: "fail", message: "'no'" }]),
      action("second", [set("s2", "note", "'two'")]),
    ];
    config.triggers = [
      trigger({ id: "t1", actionId: "first" }),
      trigger({ id: "t2", actionId: "second" }),
    ];
    await expect(insertRecord("orders", [named("status", "x")])).rejects.toThrow("Not saved: no");
  });

  it("rejects a whole write batch and writes nothing", async () => {
    config.actions = [
      action("stamp", [
        { id: "f", kind: "fail", message: "'bad row'", when: "record.status = 'bad'" },
        set("s1", "note", "'ok'"),
      ]),
    ];
    config.triggers = [trigger({})];
    const insert = (status: string) => ({
      operation: "insert" as const,
      table: "orders",
      values: [named("status", status)],
      identity: null,
    });
    await expect(writeRecordBatch([insert("good"), insert("bad")])).rejects.toThrow(
      "Not saved: bad row",
    );
    expect(db.rows("orders")).toHaveLength(1);
    expect(db.calls.some((c) => c.command === "execute_write_batch")).toBe(false);

    await writeRecordBatch([insert("good"), insert("fine")]);
    expect(db.rows("orders").map((r) => r.note)).toEqual([null, "ok", "ok"]);
  });
});

describe("deleted", () => {
  it("runs sync triggers after the delete with the deleted row as record and old", async () => {
    config.triggers = [trigger({ event: "deleted", actionId: "audit" })];
    await deleteRecord("orders", [int(1)]);
    expect(db.rows("orders")).toHaveLength(0);
    expect(db.rows("audit").map((r) => r.message)).toEqual(["deleted 1 open old=open"]);
  });

  it("uses meta.old when the caller passes it", async () => {
    config.triggers = [trigger({ event: "deleted", actionId: "audit" })];
    await deleteRecord("orders", [int(1)], { old: { id: 1, status: "passed" } });
    expect(db.rows("audit").map((r) => r.message)).toEqual(["deleted 1 passed old=passed"]);
  });

  it("honors the trigger condition against the deleted row", async () => {
    config.triggers = [
      trigger({ event: "deleted", actionId: "audit", condition: "old.status = 'x'" }),
    ];
    await deleteRecord("orders", [int(1)]);
    expect(db.rows("audit")).toHaveLength(0);
  });

  it("enqueues a job for async triggers with event deleted and the row", async () => {
    config.triggers = [trigger({ event: "deleted", actionId: "audit", mode: "async" })];
    await deleteRecord("orders", [int(1)]);
    expect(db.jobs).toHaveLength(1);
    expect(db.jobs[0]).toMatchObject({
      triggerId: "t1",
      actionId: "audit",
      payload: {
        table: "orders",
        event: "deleted",
        record: { id: 1, status: "open" },
        old: { id: 1, status: "open" },
        identity: [1],
      },
    });
    expect(db.rows("audit")).toHaveLength(0);
  });
});

describe("action queries", () => {
  const pendingRows = [1, 2].map((id) => ({
    identity: [int(id)],
    values: [
      { column: "id", value: int(id) },
      { column: "status", value: text("paid") },
    ],
    old: [
      { column: "id", value: int(id) },
      { column: "status", value: text("open") },
    ],
  }));
  const run = (extra: object = {}) => ({
    changed: 2,
    removed: 0,
    dryRun: false,
    table: "orders",
    created: [],
    updated: [],
    createdGrant: null,
    updatedGrant: null,
    ...extra,
  });
  const queryCalls = () => db.calls.filter((c) => c.command === "run_action_query");

  beforeEach(() => {
    db.addTable(
      "orders",
      ["id", "status", "note"],
      [
        { id: 1, status: "open", note: null },
        { id: 2, status: "open", note: null },
      ],
    );
    db.on("release_trigger_grant", () => null);
  });

  it("runs beforeBulk hooks on pending rows and sends overrides with the fingerprint", async () => {
    config.triggers = [trigger({})];
    db.on("run_action_query", (args) =>
      args.before
        ? run({
            updated: pendingRows.map((r) => ({ identity: r.identity, old: r.old })),
          })
        : run({ dryRun: true, pending: { fingerprint: "fp-1", rows: pendingRows } }),
    );
    const result = await runActionQuery("pay", []);
    expect(result.dryRun).toBe(false);
    expect(queryCalls()).toHaveLength(2);
    expect(queryCalls()[0].args.before).toBeUndefined();
    expect(queryCalls()[1].args.before).toEqual({
      fingerprint: "fp-1",
      overrides: [
        { row: 0, values: [{ column: "note", value: text("stamped paid") }] },
        { row: 1, values: [{ column: "note", value: text("stamped paid") }] },
      ],
    });
  });

  it("gives each pending row's old values to the trigger", async () => {
    config.actions = [action("stamp", [set("s1", "note", "old.status & '>' & record.status")])];
    config.triggers = [trigger({})];
    db.on("run_action_query", (args) =>
      args.before
        ? run()
        : run({ dryRun: true, pending: { fingerprint: "fp", rows: pendingRows } }),
    );
    await runActionQuery("pay", []);
    const overrides = (queryCalls()[1].args.before as { overrides: { values: unknown[] }[] })
      .overrides;
    expect(overrides.map((o) => o.values)).toEqual([
      [{ column: "note", value: text("open>paid") }],
      [{ column: "note", value: text("open>paid") }],
    ]);
  });

  it("sends no overrides entry for rows the triggers leave alone", async () => {
    config.triggers = [trigger({ condition: "record.id = 2" })];
    db.on("run_action_query", (args) =>
      args.before
        ? run()
        : run({ dryRun: true, pending: { fingerprint: "fp", rows: pendingRows } }),
    );
    await runActionQuery("pay", []);
    const before = queryCalls()[1].args.before as { overrides: { row: number }[] };
    expect(before.overrides.map((o) => o.row)).toEqual([1]);
  });

  it("makes no second call when a trigger rejects", async () => {
    config.actions = [
      action("stamp", [{ id: "f", kind: "fail", message: "'No paying'", when: "record.id = 2" }]),
    ];
    config.triggers = [trigger({})];
    db.on("run_action_query", () =>
      run({ dryRun: true, pending: { fingerprint: "fp", rows: pendingRows } }),
    );
    await expect(runActionQuery("pay", [])).rejects.toThrow("Not saved: No paying");
    expect(queryCalls()).toHaveLength(1);
  });

  it("does not run before-change hooks on a dry run", async () => {
    config.triggers = [trigger({})];
    db.on("run_action_query", (args) => run({ dryRun: !!args.dryRun }));
    const result = await runActionQuery("pay", [], { dryRun: true });
    expect(result.dryRun).toBe(true);
    expect(queryCalls()).toHaveLength(1);
  });

  it("fires deleted triggers once per deleted row and releases the grant once", async () => {
    config.triggers = [trigger({ event: "deleted", actionId: "audit" })];
    db.on("run_action_query", () =>
      run({
        removed: 2,
        changed: 0,
        deleted: pendingRows.map((r) => ({ identity: r.identity, values: r.old })),
        deletedGrant: "grant-d",
      }),
    );
    await runActionQuery("purge", []);
    expect(db.rows("audit").map((r) => r.message)).toEqual([
      "deleted 1 open old=open",
      "deleted 2 open old=open",
    ]);
    const released = db.calls.filter((c) => c.command === "release_trigger_grant");
    expect(released.map((c) => c.args.grant)).toEqual(["grant-d"]);
  });

  it("enqueues one async deleted job per deleted row", async () => {
    config.triggers = [trigger({ event: "deleted", actionId: "audit", mode: "async" })];
    db.on("run_action_query", () =>
      run({ deleted: pendingRows.map((r) => ({ identity: r.identity, values: r.old })) }),
    );
    await runActionQuery("purge", []);
    expect(db.jobs.map((j) => (j.payload as { identity: unknown[] }).identity)).toEqual([[1], [2]]);
    expect(db.jobs.every((j) => (j.payload as { event: string }).event === "deleted")).toBe(true);
  });
});
