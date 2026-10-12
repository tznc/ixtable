import { describe, expect, it } from "vitest";
import {
  applyMask,
  maskIssue,
  maskProblem,
  maskTemplate,
  maskedText,
  parseMask,
} from "../../src/fields/mask";

const phone = parseMask("(000) 000-0000");

describe("input masks", () => {
  it("formats typed digits and stores them without literals by default", () => {
    expect(applyMask(phone, "5551234567")).toEqual({
      display: "(555) 123-4567",
      stored: "5551234567",
      complete: true,
      rejected: 0,
    });
    expect(maskTemplate(phone)).toBe("(___) ___-____");
  });

  it("stores literals with section 0 and uses the given placeholder", () => {
    const mask = parseMask("000-0000;0;#");
    expect(applyMask(mask, "5551234").stored).toBe("555-1234");
    expect(maskTemplate(mask)).toBe("###-####");
  });

  it("reformats text that already holds the literals", () => {
    expect(applyMask(phone, "(555) 123-4567").display).toBe("(555) 123-4567");
    expect(applyMask(phone, "555-123-4567").stored).toBe("5551234567");
  });

  it("leaves trailing literals off a partial value so backspace is never stuck", () => {
    expect(applyMask(phone, "555").display).toBe("(555");
    expect(applyMask(phone, "5551").display).toBe("(555) 1");
    expect(applyMask(phone, "555").complete).toBe(false);
  });

  it("applies case conversion and letter, optional, and escaped slots", () => {
    const postcode = parseMask(">LL0 0LL");
    expect(applyMask(postcode, "sw1a").display).toBe("SW1");
    expect(applyMask(postcode, "sw1 2ab").display).toBe("SW1 2AB");
    const optional = parseMask('999\\-"x"0');
    expect(applyMask(optional, "7").display).toBe("7");
    expect(applyMask(optional, "12-x3").display).toBe("12-x3");
  });

  it("splits sections only outside quotes and escapes", () => {
    expect(parseMask('"a;b"0;0').storeLiterals).toBe(true);
    expect(parseMask('"a;b"0;0').slots).toHaveLength(4);
    expect(parseMask("0\\;0").slots).toHaveLength(3);
  });

  it("reports values that do not fill the mask and passes blanks", () => {
    expect(maskProblem("000-0000", "", "Phone")).toBeNull();
    expect(maskProblem("000-0000", null, "Phone")).toBeNull();
    expect(maskProblem("000-0000", "555", "Phone")).toBe("Phone must match ___-____.");
    expect(maskProblem("000-0000", "555-12x4", "Phone")).toBe("Phone must match ___-____.");
    expect(maskProblem("000-0000", "5551234", "Phone")).toBeNull();
    expect(maskedText("000-0000", "555-1234", "Phone")).toBe("5551234");
    expect(() => maskedText("000-0000", "1", "Phone")).toThrow("Phone must match");
  });

  it("flags a mask with nothing to type into", () => {
    expect(maskIssue("(--)")).toMatch(/no characters/);
    expect(maskIssue("0")).toBeNull();
  });
});
