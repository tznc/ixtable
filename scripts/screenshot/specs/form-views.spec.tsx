import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { captureDocument } from "../capture";
import {
  createTable,
  insert,
  LONG,
  openMode,
  refreshDatabase,
  renderNewDocument,
  saveAndReopen,
  type User,
} from "./fixtures";

type Config = {
  design: {
    forms: Array<Record<string, unknown> & { id: string }>;
    navigation: Array<Record<string, unknown>>;
  };
  actions: Array<Record<string, unknown>>;
} & Record<string, unknown>;

const id = () => crypto.randomUUID();
const at = (row: number, column: number, columnSpan: number) => ({
  column,
  row,
  columnSpan,
  rowSpan: 1,
});
const grid = () => ({
  columns: Array.from({ length: 12 }, () => ({ kind: "fr", value: 1 })),
  rows: [],
  columnGap: 16,
  rowGap: 12,
  padding: 0,
  justifyItems: "stretch",
  alignItems: "stretch",
  namedRegions: [],
  breakpoints: [],
});
const field = (label: string, column: string, kind: string, placement: unknown) => ({
  id: id(),
  kind,
  label,
  binding: { column },
  validation: { required: column === "title" },
  placement,
});
// One row: title, owner and priority side by side, as an Access continuous form lays them out.
const rowControls = () => [
  field("Title", "title", "text", at(1, 1, 6)),
  field("Owner", "owner", "text", at(1, 7, 4)),
  field("Priority", "priority", "number", at(1, 11, 2)),
];

async function seedTasks() {
  await createTable("tasks", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "title", declaredType: "TEXT" },
    { name: "owner", declaredType: "TEXT" },
    { name: "priority", declaredType: "INTEGER" },
  ]);
  const rows: Array<[string, string, number]> = [
    ["Draft launch plan", "Ana", 1],
    ["Review supplier contract", "Ben", 2],
    ["Order shelving", "Chen", 3],
    ["Update price list", "Ana", 2],
  ];
  for (const [index, [title, owner, priority]] of rows.entries())
    await insert("tasks", { id: index + 1, title, owner, priority });
  await refreshDatabase();
}

async function seedDesign(
  user: User,
  file: string,
  build: (config: Config, formId: string) => void,
) {
  const config = await invoke<Config>("read_document_config", { windowLabel: "main" });
  build(config, config.design.forms[0].id);
  await invoke("update_document_config", { windowLabel: "main", config });
  await saveAndReopen(user, file);
}

function taskForm(config: Config, formId: string, patch: Record<string, unknown>) {
  config.design.forms[0] = {
    ...config.design.forms[0],
    name: "Tasks",
    source: { kind: "table", table: "tasks" },
    layout: grid(),
    listColumns: ["title", "owner", "priority"],
    rules: [],
    controls: rowControls(),
    ...patch,
  };
  config.design.navigation[0] = {
    ...config.design.navigation[0],
    label: "Tasks",
    kind: "form",
    targetId: formId,
    mode: null,
  };
}

async function openTasks(user: User) {
  await openMode(user, "Runtime");
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Tasks" }));
  return page;
}

const titleIn = (scope: HTMLElement) => within(scope).getByRole("textbox", { name: /Title/ });

it("captures a continuous form with an edited row and the new-record row", async () => {
  const user = await renderNewDocument();
  await seedTasks();
  await seedDesign(user, "qa-continuous.ixt", (config, formId) =>
    taskForm(config, formId, { modes: ["continuous", "create", "edit"] }),
  );
  const page = await openTasks(user);
  const view = await within(page).findByRole("region", { name: "Tasks" }, LONG);
  const second = await within(view).findByRole("group", { name: "Record 2" }, LONG);
  await waitFor(() => expect(titleIn(second)).toHaveValue("Review supplier contract"), LONG);
  const fresh = within(view).getByRole("group", { name: "New record" });
  await user.type(titleIn(fresh), "Book delivery van");
  await user.clear(titleIn(second));
  await user.type(titleIn(second), "Sign supplier contract");
  expect(within(second).getByRole("button", { name: "Save" })).toBeInTheDocument();
  await captureDocument(document, {
    name: "form-views-01-continuous",
    expectations: [
      "Four task records repeat as rows with Title, Owner and Priority inputs side by side.",
      "Record 2 shows 'Sign supplier contract' with a marked left edge and Save and Undo buttons.",
      "The last row is a new-record row with 'Book delivery van' typed and an Add button.",
    ],
  });
  await user.click(within(second).getByRole("button", { name: "Save" }));
  await within(view).findByText("Changes saved.", {}, LONG);
  await captureDocument(document, {
    name: "form-views-02-continuous-saved",
    expectations: [
      "A 'Changes saved.' status shows above the rows.",
      "Record 2 shows 'Sign supplier contract' without Save or Undo buttons.",
      "The new-record row still holds 'Book delivery van' with its Add button.",
    ],
  });
});

it("captures a split form with the navigation bar", async () => {
  const user = await renderNewDocument();
  await seedTasks();
  await seedDesign(user, "qa-split.ixt", (config, formId) =>
    taskForm(config, formId, {
      modes: ["split", "detail", "create", "edit"],
      navigationBar: true,
      controls: [
        field("Title", "title", "text", at(1, 1, 12)),
        field("Owner", "owner", "text", at(2, 1, 6)),
        field("Priority", "priority", "number", at(2, 7, 6)),
      ],
    }),
  );
  const page = await openTasks(user);
  await user.click(await within(page).findByRole("row", { name: "Open Order shelving" }, LONG));
  const detail = await within(page).findByRole("form", { name: "Tasks" }, LONG);
  await waitFor(() => expect(titleIn(detail)).toHaveValue("Order shelving"), LONG);
  const bar = within(page).getByRole("group", { name: "Record navigation" });
  await within(bar).findByText("Record 3 of 4", {}, LONG);
  await captureDocument(document, {
    name: "form-views-03-split",
    expectations: [
      "The task list shows as a table above a detail pane, with the 'Order shelving' row highlighted.",
      "Above the detail pane a navigation bar shows first, previous, next, last and new buttons and 'Record 3 of 4'.",
      "The detail pane shows Order shelving, Chen and priority 3 read-only with Edit and Delete buttons.",
    ],
  });
  await user.click(within(bar).getByRole("button", { name: "Last record" }));
  await waitFor(
    () =>
      expect(titleIn(within(page).getByRole("form", { name: "Tasks" }))).toHaveValue(
        "Update price list",
      ),
    LONG,
  );
  expect(within(bar).getByRole("button", { name: "Next record" })).toBeDisabled();
  await captureDocument(document, {
    name: "form-views-04-split-last",
    expectations: [
      "The 'Update price list' row is highlighted in the list.",
      "The bar reads 'Record 4 of 4' with the next and last buttons greyed out.",
    ],
  });
});

it("captures a popup form opened by an action", async () => {
  const user = await renderNewDocument();
  await seedTasks();
  const addAction = id();
  const launcher = id();
  await seedDesign(user, "qa-popup.ixt", (config, formId) => {
    taskForm(config, formId, {
      modes: ["detail", "create", "edit"],
      controls: [
        field("Title", "title", "text", at(1, 1, 12)),
        field("Owner", "owner", "text", at(2, 1, 6)),
        field("Priority", "priority", "number", at(2, 7, 6)),
      ],
    });
    config.design.forms.push({
      id: launcher,
      name: "Planner",
      source: null,
      modes: ["create"],
      layout: grid(),
      listColumns: [],
      pageSize: 25,
      rules: [],
      controls: [
        {
          id: id(),
          kind: "label",
          label: "Intro",
          text: "Plan this week's work.",
          validation: { required: false },
          placement: at(1, 1, 12),
        },
        {
          id: id(),
          kind: "button",
          label: "Add a task",
          validation: { required: false },
          placement: at(2, 1, 3),
          actionId: addAction,
        },
      ],
    });
    config.design.navigation[0] = {
      ...config.design.navigation[0],
      label: "Tasks",
      kind: "form",
      targetId: launcher,
      mode: "create",
    };
    config.actions = [
      {
        id: addAction,
        name: "Add a task",
        onError: "stop",
        steps: [
          { id: id(), kind: "openForm", formId, mode: "create", popup: true, storeAs: "task" },
          {
            id: id(),
            kind: "message",
            text: "'Added ' & results.task.title & ' for ' & results.task.owner",
          },
        ],
      },
    ];
  });
  const page = await openTasks(user);
  const planner = await within(page).findByRole("form", { name: "New Planner" }, LONG);
  await user.click(within(planner).getByRole("button", { name: "Add a task" }));
  const popup = await screen.findByRole("dialog", { name: "Tasks" }, LONG);
  await user.type(titleIn(popup), "Call the landlord");
  await user.type(within(popup).getByRole("textbox", { name: /Owner/ }), "Ben");
  await captureDocument(document, {
    name: "form-views-05-popup",
    expectations: [
      "A modal dialog titled 'New Tasks' sits over a dimmed Planner page.",
      "The dialog shows Title 'Call the landlord', Owner 'Ben', and Cancel, Create and Close buttons.",
    ],
  });
  await user.click(within(popup).getByRole("button", { name: "Create" }));
  await within(planner).findByText("Added Call the landlord for Ben", {}, LONG);
  expect(screen.queryByRole("dialog")).toBeNull();
  await captureDocument(document, {
    name: "form-views-06-popup-result",
    expectations: [
      "The dialog is gone and the Planner page shows 'Added Call the landlord for Ben'.",
    ],
  });
});

it("captures the form properties with the new modes and the navigation bar setting", async () => {
  const user = await renderNewDocument();
  await seedTasks();
  await seedDesign(user, "qa-props.ixt", (config, formId) =>
    taskForm(config, formId, {
      modes: ["list", "detail", "create", "edit", "continuous"],
      navigationBar: true,
    }),
  );
  await openMode(user, "Design");
  await screen.findByRole("region", { name: "Form builder" }, LONG);
  const properties = await screen.findByRole("complementary", { name: "Properties" }, LONG);
  const bar = await within(properties).findByRole(
    "checkbox",
    { name: "Record navigation bar" },
    LONG,
  );
  expect(bar).toBeChecked();
  expect(within(properties).getByRole("checkbox", { name: "Continuous mode" })).toBeChecked();
  expect(within(properties).getByRole("checkbox", { name: "Split mode" })).not.toBeChecked();
  await captureDocument(document, {
    name: "form-views-07-properties",
    expectations: [
      "The Properties panel's Modes section lists List, Detail, Create, Edit, Continuous and Split mode checkboxes.",
      "Continuous mode and 'Record navigation bar' are checked; Split mode is not.",
    ],
  });
});
