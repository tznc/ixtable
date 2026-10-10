import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import type { DocumentConfig } from "../../src/lib/types";
import { createTable, readPage, renderNewDocument } from "./helpers";

const LONG = { timeout: 20_000 };
type Ref = { id: string; name: string; mime: string; size: number; sha256: string };

async function docs() {
  await createTable("docs", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "phone", declaredType: "TEXT" },
    { name: "notes", declaredType: "TEXT" },
    { name: "tags", declaredType: "TEXT" },
    { name: "files", declaredType: "TEXT" },
  ]);
}

const readConfig = () => invoke<DocumentConfig>("read_document_config", { windowLabel: "main" });

it("stores attachment files in the record store and reads them back by id", async () => {
  await renderNewDocument();
  await docs();
  const config = await readConfig();
  const entities = config.entities.map((e) =>
    e.table === "docs"
      ? { ...e, fields: [{ id: "f1", column: "files", format: "attachment" }] }
      : e,
  );
  await invoke("update_document_config", { windowLabel: "main", config: { ...config, entities } });
  const upload = (column: string) =>
    invoke<Ref>("upload_record_attachment", {
      windowLabel: "main",
      table: "docs",
      column,
      name: "../notes.txt",
      mime: null,
      contentBase64: btoa("hello"),
    });
  const file = await upload("files");
  expect(file).toMatchObject({ name: "notes.txt", mime: "text/plain", size: 5 });
  const content = await invoke<Ref & { contentBase64: string }>("read_record_attachment", {
    windowLabel: "main",
    id: file.id,
  });
  expect(atob(content.contentBase64)).toBe("hello");
  await expect(upload("notes")).rejects.toThrow(/notes is not an attachment field/);
  const removed = await invoke<number>("remove_unused_record_attachments", {
    windowLabel: "main",
  });
  expect(removed).toBe(0);
});

it("sets field formats in the table designer and enters each kind in a generated form", async () => {
  const user = await renderNewDocument();
  await docs();
  await user.click(await screen.findByRole("button", { name: "Design table" }, LONG));
  const fields = await screen.findByRole("region", { name: "Field settings" }, LONG);
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "notes format" }),
    "richText",
  );
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "tags format" }),
    "multiSelect",
  );
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "files format" }),
    "attachment",
  );
  await user.type(
    within(fields).getByRole("textbox", { name: /phone input mask/ }),
    "(000) 000-0000",
  );
  const choices = await within(fields).findByRole("textbox", { name: "tags choices" });
  await user.type(choices, "red{Enter}blue");
  await user.tab();
  await waitFor(async () => {
    const entity = (await readConfig()).entities.find((e) => e.table === "docs");
    expect(entity?.fields?.map((f) => [f.column, f.format ?? f.inputMask])).toEqual(
      expect.arrayContaining([
        ["notes", "richText"],
        ["tags", "multiSelect"],
        ["files", "attachment"],
        ["phone", "(000) 000-0000"],
      ]),
    );
    expect(entity?.fields?.find((f) => f.column === "tags")?.options).toEqual(["red", "blue"]);
  }, LONG);

  await user.click(screen.getByRole("button", { name: "Design" }));
  await screen.findByRole("region", { name: "Form builder" }, LONG);
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Table to generate from" }),
    "docs",
  );
  await user.click(screen.getByRole("button", { name: "Generate form from table" }));
  const forms = screen.getByRole("region", { name: "Forms" });
  await within(forms).findByRole("button", { name: "Docs list" }, LONG);
  await user.click(screen.getByRole("button", { name: "Runtime" }));
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Docs" }));
  await user.click(await within(page).findByRole("button", { name: "New docs" }, LONG));
  const create = await screen.findByRole("form", { name: "New Docs" }, LONG);

  const phone = within(create).getByRole("textbox", { name: "Phone" });
  await user.type(phone, "5551234567");
  expect(phone).toHaveValue("(555) 123-4567");
  await user.type(within(create).getByRole("textbox", { name: "Notes" }), "Hello");
  await user.click(within(create).getByRole("checkbox", { name: "blue" }));
  await user.upload(
    within(create).getByLabelText(/Add files/),
    new File(["file body"], "a.txt", { type: "text/plain" }),
  );
  await within(create).findByText("a.txt", {}, LONG);
  await user.click(within(create).getByRole("button", { name: "Create" }));

  await waitFor(async () => expect((await readPage("docs")).rows).toHaveLength(1), LONG);
  const [row] = (await readPage("docs")).rows;
  expect(row[1]).toEqual({ type: "text", value: "5551234567" });
  expect(String(row[2].value)).toContain("Hello");
  expect(row[3]).toEqual({ type: "text", value: '["blue"]' });
  expect(JSON.parse(String(row[4].value))).toEqual([
    expect.objectContaining({ name: "a.txt", mime: "text/plain", size: 9 }),
  ]);
});

it("rejects a typed value that does not fill the input mask", async () => {
  const user = await renderNewDocument();
  await docs();
  await user.click(await screen.findByRole("button", { name: "Design table" }, LONG));
  const fields = await screen.findByRole("region", { name: "Field settings" }, LONG);
  await user.type(within(fields).getByRole("textbox", { name: /phone input mask/ }), "000-0000");
  await waitFor(async () => {
    const entity = (await readConfig()).entities.find((e) => e.table === "docs");
    expect(entity?.fields?.[0]?.inputMask).toBe("000-0000");
  }, LONG);
  await user.click(screen.getByRole("button", { name: "Close" }));
  const draft = await screen.findByRole("textbox", { name: "New phone" }, LONG);
  await user.type(draft, "12{Enter}");
  await screen.findByText("phone must match ___-____.", {}, LONG);
  await user.clear(draft);
  await user.type(draft, "5551234{Enter}");
  await waitFor(async () => expect((await readPage("docs")).rows).toHaveLength(1), LONG);
  expect((await readPage("docs")).rows[0][1]).toEqual({ type: "text", value: "5551234" });
});
