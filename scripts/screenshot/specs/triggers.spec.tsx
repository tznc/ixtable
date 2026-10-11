import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { captureDocument } from "../capture";
import { LONG, openMode, startFromTemplate, type User } from "./fixtures";

const runtimePage = () => screen.getByRole("region", { name: "Application page" });

const titles = async () =>
  (
    await invoke<{ rows: Array<Array<{ value?: unknown }>> }>("execute_read_query", {
      windowLabel: "main",
      sql: "SELECT title FROM work_orders WHERE number LIKE 'WO-2%' ORDER BY number",
    })
  ).rows.map((r) => r[0]?.value);

async function addStep(user: User, kind: string) {
  await user.selectOptions(screen.getByRole("combobox", { name: "New step kind" }), kind);
  await user.click(screen.getByRole("button", { name: "Add step" }));
}

async function newWorkOrder(user: User, number: string, title: string) {
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = await screen.findByRole("navigation", { name: "Application navigation" }, LONG);
  await user.click(await within(nav).findByRole("button", { name: "Work orders" }, LONG));
  await user.click(await within(page).findByRole("button", { name: "New work order" }, LONG));
  const create = await within(page).findByRole("form", { name: "New Work order" }, LONG);
  await user.type(within(create).getByRole("textbox", { name: "Number" }), number);
  await user.type(within(create).getByRole("textbox", { name: "Title" }), title);
  for (const [label, option] of [
    ["Asset", "Rooftop HVAC unit"],
    ["Priority", "High"],
  ]) {
    const select = await within(create).findByRole("combobox", { name: label }, LONG);
    await within(select).findByRole("option", { name: option }, LONG);
    await user.selectOptions(select, option);
  }
  await user.click(within(create).getByRole("button", { name: "Create" }));
  return create;
}

it("sets fields and rejects saves with a before-change trigger, and edits a deleted trigger", async () => {
  const user = await startFromTemplate("Work orders");
  await openMode(user, "Automation");
  await screen.findByRole("heading", { name: "Automation" }, LONG);

  // An action that upper-cases the title and refuses short titles.
  await user.click(screen.getByRole("tab", { name: "Actions" }));
  await user.click(await screen.findByRole("button", { name: "New action" }, LONG));
  const name = await screen.findByRole("textbox", { name: "Action name" }, LONG);
  await user.clear(name);
  await user.type(name, "Check work order");
  await addStep(user, "fail");
  const fail = await screen.findByRole("group", { name: "Step 1: Fail with message" }, LONG);
  await user.type(
    within(fail).getByRole("textbox", { name: "Failure message" }),
    "'Titles need at least 5 characters'",
  );
  await user.type(
    within(fail).getByRole("textbox", { name: "Only when" }),
    "len(record.title) < 5",
  );
  await addStep(user, "setField");
  const setField = await screen.findByRole(
    "group",
    { name: "Step 2: Set field (before-change)" },
    LONG,
  );
  await user.type(within(setField).getByRole("textbox", { name: "Field" }), "title");
  await user.type(within(setField).getByRole("textbox", { name: "Value" }), "upper(record.title)");
  await waitFor(
    () =>
      expect(within(setField).getByRole("textbox", { name: "Value" })).toHaveValue(
        "upper(record.title)",
      ),
    LONG,
  );
  await captureDocument(document, {
    name: "triggers-01-set-field-action",
    expectations: [
      "The Check work order action has a Fail with message step guarded by len(record.title) < 5.",
      "Its second step is 'Set field (before-change)' setting title to upper(record.title).",
    ],
  });

  // A before-change trigger on work_orders that runs it.
  await user.click(screen.getByRole("tab", { name: "Triggers" }));
  await user.click(await screen.findByRole("button", { name: "New trigger" }, LONG));
  const triggerName = await screen.findByRole("textbox", { name: "Trigger name" }, LONG);
  await user.clear(triggerName);
  await user.type(triggerName, "Check work orders before saving");
  await user.selectOptions(screen.getByRole("combobox", { name: "Table" }), "work_orders");
  await user.selectOptions(screen.getByRole("combobox", { name: "Event" }), "beforeChange");
  await user.selectOptions(screen.getByRole("combobox", { name: "Action" }), "Check work order");
  expect(screen.queryByLabelText("Run")).toBeNull();
  expect(screen.getByText(/Runs before each new or changed record is saved/)).toBeInTheDocument();
  expect(screen.queryByText(/Sync triggers run after the record is saved/)).toBeNull();
  expect(
    within(screen.getByRole("combobox", { name: "Event" }))
      .getAllByRole("option")
      .map((o) => o.textContent),
  ).toEqual(["Before a record is saved", "Record created", "Record updated", "Record deleted"]);
  await captureDocument(document, {
    name: "triggers-02-before-change-editor",
    expectations: [
      "The new trigger's Event reads 'Before a record is saved' on table work_orders with action Check work order.",
      "The sync/async Run select is gone; the only help text says it can set fields or reject the save and cannot write records.",
    ],
  });

  // Runtime: a short title is refused with the trigger's message and nothing is saved.
  await openMode(user, "Runtime");
  const refused = await newWorkOrder(user, "WO-2001", "Fix");
  await waitFor(
    () =>
      expect(
        within(refused)
          .queryAllByRole("alert")
          .map((a) => a.textContent)
          .join("\n"),
      ).toMatch("Not saved: Titles need at least 5 characters"),
    LONG,
  );
  expect(await titles()).toEqual([]);
  await captureDocument(document, {
    name: "triggers-03-save-rejected",
    expectations: [
      "The New Work order form stays open with the error 'Not saved: Titles need at least 5 characters'.",
      "The entered number WO-2001 and title Fix are still in the form.",
    ],
  });

  // A valid title is saved upper-cased by the Set field step.
  await newWorkOrder(user, "WO-2002", "replace hvac belt");
  const saved = await within(runtimePage()).findByRole("form", { name: "Work order" }, LONG);
  await waitFor(
    () =>
      expect(within(saved).getByRole("textbox", { name: "Title" })).toHaveValue(
        "REPLACE HVAC BELT",
      ),
    LONG,
  );
  expect(await titles()).toEqual(["REPLACE HVAC BELT"]);
  await captureDocument(document, {
    name: "triggers-04-field-set-on-save",
    expectations: [
      "The saved work order WO-2002 shows its title as REPLACE HVAC BELT, upper-cased by the trigger.",
    ],
  });

  // A record-deleted trigger keeps the Run and Run as options of an after trigger.
  await openMode(user, "Automation");
  await user.click(await screen.findByRole("tab", { name: "Triggers" }, LONG));
  await user.click(await screen.findByRole("button", { name: "New trigger" }, LONG));
  const deletedName = await screen.findByRole("textbox", { name: "Trigger name" }, LONG);
  await user.clear(deletedName);
  await user.type(deletedName, "Log deleted work orders");
  await user.selectOptions(screen.getByRole("combobox", { name: "Table" }), "work_orders");
  await user.selectOptions(screen.getByRole("combobox", { name: "Event" }), "deleted");
  await user.selectOptions(screen.getByRole("combobox", { name: "Action" }), "Log new work order");
  expect(screen.getByLabelText("Run")).toHaveValue("sync");
  await captureDocument(document, {
    name: "triggers-05-deleted-editor",
    expectations: [
      "The new trigger's Event reads 'Record deleted' on table work_orders with action Log new work order.",
      "Run as and the sync/async Run select are shown, and the help text says a deleted record is seen as it was.",
    ],
  });
});
