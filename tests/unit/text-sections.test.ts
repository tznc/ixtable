import { expect, it } from "vitest";
import { textSections } from "../../src/persistence/textSections";

it("splits the Access VBA asset into one section per module and form", () => {
  const text =
    "' ==== Module Helpers ====\r\nOption Compare Database\r\nPublic Function Twice(x)\r\n  Twice = x * 2\r\nEnd Function\r\n\r\n' ==== Form Customer Details ====\r\nPrivate Sub Form_Load()\r\nEnd Sub\r\n";
  expect(textSections(text)).toEqual([
    {
      title: "Module Helpers",
      text: "Option Compare Database\nPublic Function Twice(x)\n  Twice = x * 2\nEnd Function",
    },
    { title: "Form Customer Details", text: "Private Sub Form_Load()\nEnd Sub" },
  ]);
});

it("keeps text without section lines as one untitled section", () => {
  expect(textSections("\nplain notes\nsecond line\n")).toEqual([
    { title: "", text: "plain notes\nsecond line" },
  ]);
  expect(textSections("")).toEqual([]);
});
