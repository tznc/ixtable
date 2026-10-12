import { describe, expect, it, vi } from "vitest";
import type { ActionContext } from "../../src/automation/runner";
import { upgradeDesign } from "../../src/design/schema";
import type { DesignForm } from "../../src/design/schema";
import type { DocumentConfig } from "../../src/lib/types";
import {
  afterUpdateProblem,
  eventAction,
  eventsToRaise,
  runFormEvent,
  vetoMessage,
} from "../../src/runtime/formEvents";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const form = (events: DesignForm["events"]): DesignForm =>
  ({ id: "f", name: "Orders", events }) as DesignForm;

const config = {
  actions: [
    {
      id: "guard",
      name: "Guard",
      onError: "stop",
      steps: [
        { id: "s1", kind: "fail", when: "record.status = 'bad'", message: "'No bad orders'" },
        { id: "s2", kind: "message", text: "'ok ' & record.status" },
      ],
    },
  ],
  triggers: [],
  savedQueries: [],
  design: { version: 3, forms: [], navigation: [] },
  reports: [],
  dashboards: [],
} as unknown as DocumentConfig;

const ctx = (record: Record<string, unknown>, extra: Partial<ActionContext> = {}) => {
  const notes: string[] = [];
  const context: ActionContext = {
    config,
    record,
    app: {},
    navigate: () => undefined,
    setState: () => undefined,
    confirm: async () => true,
    notify: (text) => notes.push(text),
    ...extra,
  };
  return { context, notes };
};

describe("form events", () => {
  it("raises on load once per view and on current per record", () => {
    const state = { opened: false, current: null as string | null };
    expect(eventsToRaise(state, "a")).toEqual(["onLoad", "onCurrent"]);
    expect(eventsToRaise(state, "a")).toEqual([]);
    expect(eventsToRaise(state, "b")).toEqual(["onCurrent"]);
  });

  it("runs nothing for an unbound event", async () => {
    expect(eventAction(form({}), "onLoad")).toBeNull();
    expect(eventAction(form({ onLoad: "" }), "onLoad")).toBeNull();
    expect(await runFormEvent(form(undefined), "beforeUpdate", ctx({}).context)).toBeNull();
  });

  it("vetoes a save when the before update action fails", async () => {
    const bound = form({ beforeUpdate: "guard" });
    const bad = await runFormEvent(bound, "beforeUpdate", ctx({ status: "bad" }).context);
    expect(vetoMessage(bad)).toBe("No bad orders");
    const good = ctx({ status: "paid" });
    const ok = await runFormEvent(bound, "beforeUpdate", good.context);
    expect(vetoMessage(ok)).toBeNull();
    expect(good.notes).toEqual(["ok paid"]);
  });

  it("vetoes when the role cannot run the action, and on a declined confirm", async () => {
    const bound = form({ beforeUpdate: "guard" });
    const denied = await runFormEvent(
      bound,
      "beforeUpdate",
      ctx({ status: "paid" }, { authorize: () => false }).context,
    );
    expect(vetoMessage(denied)).toBe("Not permitted");
    expect(vetoMessage({ ok: false, cancelled: true, steps: [], results: {} })).toBe(
      "Save cancelled.",
    );
    expect(vetoMessage(null)).toBeNull();
  });

  it("reports a failed after update action without undoing the save", () => {
    expect(afterUpdateProblem({ ok: false, error: "boom", steps: [], results: {} })).toBe(
      "Saved, but the after update action failed: boom",
    );
    expect(afterUpdateProblem({ ok: true, steps: [], results: {} })).toBeNull();
    expect(afterUpdateProblem({ ok: false, cancelled: true, steps: [], results: {} })).toBeNull();
  });

  it("keeps bound events through a design upgrade and drops empty ones", () => {
    const design = upgradeDesign({
      version: 3,
      forms: [
        { id: "a", name: "A", events: { onLoad: "x", onCurrent: "", afterUpdate: 3 } },
        { id: "b", name: "B" },
      ],
    });
    expect(design.forms[0].events).toEqual({ onLoad: "x" });
    expect("events" in design.forms[1]).toBe(false);
  });
});
