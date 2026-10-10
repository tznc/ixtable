import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, expect, it } from "vitest";
import {
  AttachmentInput,
  MaskedInput,
  MultiSelectInput,
  RichTextInput,
} from "../../src/fields/inputs";

let user: ReturnType<typeof userEvent.setup>;
beforeEach(() => {
  user = userEvent.setup();
});

const changes: unknown[] = [];
function Harness({ kind, initial = null }: { kind: string; initial?: unknown }) {
  const [value, setValue] = useState<unknown>(initial);
  const common = {
    id: "f",
    label: "Field",
    value,
    onBlur: () => undefined,
    disabled: false,
    onChange: (next: unknown) => {
      changes.push(next);
      setValue(next);
    },
  };
  if (kind === "mask") return <MaskedInput {...common} mask="(000) 000-0000" />;
  if (kind === "rich") return <RichTextInput {...common} />;
  if (kind === "readonly") return <RichTextInput {...common} disabled />;
  return <MultiSelectInput {...common} options={["red", "blue"]} />;
}

it("formats masked entry as it is typed and stores the typed characters", async () => {
  render(<Harness kind="mask" />);
  const input = screen.getByRole("textbox");
  expect(input).toHaveAttribute("placeholder", "(___) ___-____");
  await user.type(input, "5551234567");
  expect(input).toHaveValue("(555) 123-4567");
  expect(changes.at(-1)).toBe("5551234567");
});

it("emits sanitized HTML from the rich-text box", async () => {
  render(<Harness kind="rich" />);
  const box = screen.getByRole("textbox", { name: "Field" });
  await user.type(box, "Hello");
  expect(changes.at(-1)).toBe("Hello");
  expect(screen.getByRole("toolbar", { name: "Field formatting" })).toBeInTheDocument();
});

it("shows read-only rich text without scripts", () => {
  render(<Harness kind="readonly" initial={"<b>Bold</b><script>alert(1)</script>"} />);
  const view = screen.getByLabelText("Field");
  expect(view.innerHTML).toBe("<b>Bold</b>");
});

it("toggles choices into a JSON array and keeps unknown stored choices", async () => {
  render(<Harness kind="multi" initial={'["green"]'} />);
  expect(screen.getByRole("checkbox", { name: "green" })).toBeChecked();
  await user.click(screen.getByRole("checkbox", { name: "blue" }));
  expect(changes.at(-1)).toBe('["blue","green"]');
  await user.click(screen.getByRole("checkbox", { name: "green" }));
  await user.click(screen.getByRole("checkbox", { name: "blue" }));
  expect(changes.at(-1)).toBeNull();
});

it("lists attachment files and removes one", async () => {
  const files = [
    { id: "a", name: "a.txt", mime: "text/plain", size: 10, sha256: "x" },
    { id: "b", name: "b.txt", mime: "text/plain", size: 2000, sha256: "y" },
  ];
  let value: unknown = JSON.stringify(files);
  render(
    <AttachmentInput
      id="f"
      label="Files"
      value={value}
      onChange={(next) => {
        value = next;
      }}
      onBlur={() => undefined}
      disabled={false}
      table="docs"
      column="files"
    />,
  );
  expect(screen.getByText("2 KB")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Remove a.txt" }));
  expect(JSON.parse(String(value))).toEqual([files[1]]);
});
