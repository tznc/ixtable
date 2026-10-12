import { describe, expect, it } from "vitest";
import {
  embeddableForms,
  MAX_SUBFORM_DEPTH,
  subformDepth,
  subformHeight,
} from "../../src/design/nesting";
import { type DesignForm, newControl, newForm } from "../../src/design/schema";

function form(id: string, ...children: string[]): DesignForm {
  const base = { ...newForm(id), id };
  const controls = children.map((child) => ({
    ...newControl("relatedList", base),
    related: { table: "t", foreignKey: "p", parentColumn: "id", columns: [], formId: child },
  }));
  return { ...base, controls };
}

describe("related list nesting", () => {
  it("measures levels below and above a form", () => {
    const forms = [form("a", "b"), form("b", "c", "x"), form("c", "d"), form("d"), form("x")];
    expect(MAX_SUBFORM_DEPTH).toBe(3);
    expect(subformDepth(forms, "a")).toBe(3);
    expect(subformDepth(forms, "d")).toBe(0);
    expect(subformHeight(forms, "d")).toBe(3);
    expect(subformHeight(forms, "a")).toBe(0);
    const looped = [form("a", "b"), form("b", "a")];
    expect(subformDepth(looped, "a")).toBe(Number.POSITIVE_INFINITY);
    expect(subformHeight(looped, "a")).toBe(Number.POSITIVE_INFINITY);
  });

  it("offers child forms that stay within three levels and never loop", () => {
    const forms = [form("a", "b"), form("b", "c"), form("c"), form("d", "e"), form("e")];
    const offered = (id: string) =>
      embeddableForms(forms, forms.find((f) => f.id === id) as DesignForm).map((f) => f.id);
    expect(offered("a")).toEqual(["b", "c", "d", "e"]);
    expect(offered("c")).toEqual(["e"]);
    expect(offered("b")).toEqual(["c", "d", "e"]);
  });
});
