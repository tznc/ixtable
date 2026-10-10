import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { captureDocument } from "../capture";
import { createTable, LONG, openMode, refreshDatabase, renderNewDocument } from "./fixtures";

type Config = { entities: Array<{ table: string; fields?: Array<{ column: string }> }> };
const fieldCount = async () =>
  (await invoke<Config>("read_document_config", { windowLabel: "main" })).entities.find(
    (e) => e.table === "contacts",
  )?.fields?.length ?? 0;

it("sets field formats in the table designer and enters each kind in the Runtime", async () => {
  const user = await renderNewDocument();
  await createTable("contacts", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "name", declaredType: "TEXT" },
    { name: "phone", declaredType: "TEXT" },
    { name: "notes", declaredType: "TEXT" },
    { name: "interests", declaredType: "TEXT" },
    { name: "documents", declaredType: "TEXT" },
  ]);
  await refreshDatabase();
  await user.click(await screen.findByRole("button", { name: /^contacts\b/ }, LONG));
  await user.click(await screen.findByRole("button", { name: "Design table" }, LONG));
  const fields = await screen.findByRole("region", { name: "Field settings" }, LONG);
  await user.type(
    within(fields).getByRole("textbox", { name: /phone input mask/ }),
    "(000) 000-0000",
  );
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "notes format" }),
    "richText",
  );
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "interests format" }),
    "multiSelect",
  );
  await user.type(
    await within(fields).findByRole("textbox", { name: "interests choices" }),
    "Pricing{Enter}Support{Enter}Training",
  );
  await user.selectOptions(
    within(fields).getByRole("combobox", { name: "documents format" }),
    "attachment",
  );
  await waitFor(async () => expect(await fieldCount()).toBe(4), LONG);
  fields.scrollIntoView();
  await captureDocument(document, {
    name: "fields-01-table-designer",
    expand: ".table-designer",
    expectations: [
      "The Field settings section lists the text columns with a Format picker each.",
      "phone shows the input mask (000) 000-0000 and its preview (___) ___-____.",
      "notes is Rich text, interests is Multiple choices with three choices, documents is Attachments.",
    ],
  });

  await openMode(user, "Design");
  await screen.findByRole("region", { name: "Form builder" }, LONG);
  await user.selectOptions(
    screen.getByRole("combobox", { name: "Table to generate from" }),
    "contacts",
  );
  await user.click(screen.getByRole("button", { name: "Generate form from table" }));
  const forms = screen.getByRole("region", { name: "Forms" });
  await within(forms).findByRole("button", { name: "Contacts list" }, LONG);
  await openMode(user, "Runtime");
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Contacts" }));
  await user.click(await within(page).findByRole("button", { name: "New contacts" }, LONG));
  const create = await screen.findByRole("form", { name: "New Contacts" }, LONG);
  await user.type(within(create).getByRole("textbox", { name: "Name" }), "Ada Byrne");
  await user.type(within(create).getByRole("textbox", { name: "Phone" }), "5551234567");
  await user.type(
    within(create).getByRole("textbox", { name: "Notes" }),
    "Asked about the annual plan.",
  );
  await user.click(within(create).getByRole("checkbox", { name: "Support" }));
  await user.upload(
    within(create).getByLabelText(/Add files/),
    new File(["%PDF-1.4 quote"], "quote.pdf", { type: "application/pdf" }),
  );
  await within(create).findByText("quote.pdf", {}, LONG);
  expect(within(create).getByRole("textbox", { name: "Phone" })).toHaveValue("(555) 123-4567");
  await captureDocument(document, {
    name: "fields-02-runtime-entry",
    expectations: [
      "The New Contacts form shows Phone formatted as (555) 123-4567.",
      "Notes is a rich-text box with a formatting toolbar, and Interests shows three checkboxes with Support ticked.",
      "Documents lists quote.pdf with its size and Save and Remove buttons, plus an Add files button.",
    ],
  });
});
