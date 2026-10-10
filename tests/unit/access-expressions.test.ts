import { describe, expect, it } from "vitest";
import { evaluate } from "../../src/expr";

// Expressions the Access importer writes for form and report controls
// (src-tauri/src/access/tests/translate_coverage.rs asserts the same text).
const record = { Amount: 42, Due: "2024-08-15", Status: "Open order", Find: "order", Priority: 2 };

describe("translated Access expressions", () => {
  it.each([
    ["contains(record.Status, record.Find)", true],
    ["startswith(record.Status, record.Find)", false],
    ["endswith(record.Status, record.Find)", true],
    ["upper(record.Status)", "OPEN ORDER"],
    ["format(date(2000, month(record.Due), 1), 'MMMM')", "August"],
    ["format(date(2000, 3, 1), 'MMM')", "Mar"],
    ["floor((month(record.Due) - 1) / 3) + 1", 3],
    ["year(record.Due)", 2024],
    [
      "if(record.Amount > 100, 'High', if(record.Amount > 10, 'Mid', if(true, 'Low', null)))",
      "Mid",
    ],
    [
      "if(record.Priority = 1, 'Low', if(record.Priority = 2, 'Normal', if(record.Priority = 3, 'High', null)))",
      "Normal",
    ],
    ["if(record.Amount > 0, 1, if(record.Amount < 0, -1, 0))", 1],
  ])("%s", (source, expected) => {
    expect(evaluate(source, { record })).toBe(expected);
  });
});
