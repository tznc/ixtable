import { describe, expect, it, vi } from "vitest";
import type { DocumentConfig } from "../../src/lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const { runAction } = await import("../../src/automation/runner");
const { CLOSE_FORM_EVENT, OPEN_POPUP_EVENT, requestClose, requestPopup } = await import(
  "../../src/runtime/popup"
);
const { DEFAULT_FORM_MODES, isCollectionMode, newForm, upgradeDesign } = await import(
  "../../src/design/schema"
);
type ActionContext = import("../../src/automation/runner").ActionContext;
type Step = import("../../src/automation/types").Step;

const config = (steps: Step[]) =>
  ({
    actions: [{ id: "a1", name: "Pick", steps, onError: "stop" }],
    triggers: [],
    savedQueries: [],
    design: { version: 3, forms: [{ id: "f1", name: "Customers" }], navigation: [] },
    reports: [],
    dashboards: [],
  }) as unknown as DocumentConfig;

function context(cfg: DocumentConfig, extra: Partial<ActionContext> = {}) {
  const events: unknown[] = [];
  const ctx: ActionContext = {
    config: cfg,
    app: {},
    navigate: (target) => events.push(["navigate", target]),
    setState: (scope, key, value) => events.push(["state", scope, key, value]),
    confirm: async () => true,
    notify: (message) => events.push(["notify", message]),
    ...extra,
  };
  return { ctx, events };
}

describe("popup forms", () => {
  it("waits for a popup and stores what it returns for later steps", async () => {
    const cfg = config([
      { id: "s1", kind: "openForm", formId: "f1", mode: "create", popup: true, storeAs: "picked" },
      { id: "s2", kind: "message", text: "'Chose ' & results.picked.name" },
    ]);
    const openPopup = vi.fn(async () => ({ id: 7, name: "Acme" }));
    const { ctx, events } = context(cfg, { openPopup });
    const result = await runAction("a1", ctx);
    expect(result.ok).toBe(true);
    expect(openPopup).toHaveBeenCalledWith({ kind: "form", id: "f1", mode: "create" });
    expect(result.results.picked).toEqual({ id: 7, name: "Acme" });
    expect(events).toEqual([["notify", "Chose Acme"]]);
  });

  it("stores null when the popup is dismissed", async () => {
    const cfg = config([
      { id: "s1", kind: "openForm", formId: "f1", popup: true, storeAs: "picked" },
    ]);
    const { ctx } = context(cfg, { openPopup: async () => null });
    const result = await runAction("a1", ctx);
    expect(result.results.picked).toBeNull();
  });

  it("opens the form as a page when no popup host is listening", async () => {
    const cfg = config([
      { id: "s1", kind: "openForm", formId: "f1", popup: true, storeAs: "picked" },
    ]);
    const { ctx, events } = context(cfg);
    const result = await runAction("a1", ctx);
    expect(result.ok).toBe(true);
    expect(result.results.picked).toBeNull();
    expect(events).toEqual([["navigate", { kind: "form", id: "f1" }]]);
  });

  it("asks the host through window events", async () => {
    const opened = vi.fn((event: Event) => {
      event.preventDefault();
      (event as CustomEvent).detail.resolve("chosen");
    });
    window.addEventListener(OPEN_POPUP_EVENT, opened);
    await expect(requestPopup({ kind: "form", id: "f1" })).resolves.toBe("chosen");
    window.removeEventListener(OPEN_POPUP_EVENT, opened);
    await expect(requestPopup({ kind: "form", id: "f1" })).resolves.toBeUndefined();

    const closed = vi.fn((event: Event) => event.preventDefault());
    window.addEventListener(CLOSE_FORM_EVENT, closed);
    expect(requestClose(3)).toBe(true);
    expect((closed.mock.calls[0][0] as CustomEvent).detail).toEqual({ value: 3 });
    window.removeEventListener(CLOSE_FORM_EVENT, closed);
    expect(requestClose(3)).toBe(false);
  });

  it("closeForm returns its value to the context", async () => {
    const cfg = config([{ id: "s1", kind: "closeForm", value: "record.id * 2" }]);
    const closeForm = vi.fn();
    const { ctx } = context(cfg, { closeForm, record: { id: 21 } });
    expect((await runAction("a1", ctx)).ok).toBe(true);
    expect(closeForm).toHaveBeenCalledWith(42);
  });

  it("closeForm is held back until a rollback action commits", async () => {
    const cfg = config([
      { id: "s1", kind: "closeForm" },
      { id: "s2", kind: "fail", message: "'stop'" },
    ]);
    cfg.actions[0].onError = "rollback";
    const closeForm = vi.fn();
    const { ctx } = context(cfg, { closeForm });
    expect((await runAction("a1", ctx)).ok).toBe(false);
    expect(closeForm).not.toHaveBeenCalled();
  });
});

describe("form modes and navigation bar", () => {
  it("keeps the four classic modes as the default and marks collection modes", () => {
    expect(newForm("Tasks").modes).toEqual(DEFAULT_FORM_MODES);
    expect(newForm("Tasks").navigationBar).toBe(true);
    expect(["list", "continuous", "split"].every((m) => isCollectionMode(m as "list"))).toBe(true);
    expect(isCollectionMode("detail")).toBe(false);
  });

  it("upgrades stored forms without the bar as off and keeps new modes", () => {
    const design = upgradeDesign({
      forms: [
        { id: "a", name: "A" },
        { id: "b", name: "B", modes: ["continuous", "split", "bogus"], navigationBar: true },
      ],
      navigation: [],
    });
    expect(design.forms[0].modes).toEqual(DEFAULT_FORM_MODES);
    expect(design.forms[0].navigationBar).toBeUndefined();
    expect(design.forms[1].modes).toEqual(["continuous", "split"]);
    expect(design.forms[1].navigationBar).toBe(true);
  });
});
