import { join } from "node:path";
import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it } from "vitest";
import { createTable, insertRow, readPage, renderNewDocument, value } from "./helpers";
import { dialogMock } from "./setup";

const LONG = { timeout: 20_000 };

type Config = {
  design: {
    forms: Array<Record<string, unknown> & { id: string }>;
    navigation: Array<Record<string, unknown>>;
  };
  actions: Array<Record<string, unknown>>;
} & Record<string, unknown>;

const id = () => crypto.randomUUID();
const at = (row: number, column = 1, columnSpan = 6) => ({ column, row, columnSpan, rowSpan: 1 });
const grid = () => ({
  columns: Array.from({ length: 12 }, () => ({ kind: "fr", value: 1 })),
  rows: [],
  columnGap: 16,
  rowGap: 16,
  padding: 0,
  justifyItems: "stretch",
  alignItems: "stretch",
  namedRegions: [],
  breakpoints: [],
});
const titleField = () => ({
  id: id(),
  kind: "text",
  label: "Title",
  binding: { column: "title" },
  validation: { required: true },
  placement: at(1),
});

type User = Awaited<ReturnType<typeof renderNewDocument>>;

async function seedTasks(titles: string[]) {
  await createTable("tasks", [
    { name: "id", declaredType: "INTEGER", primaryKeyPosition: 1 },
    { name: "title", declaredType: "TEXT" },
  ]);
  for (const [index, title] of titles.entries())
    await insertRow("tasks", [
      { column: "id", value: value("integer", index + 1) },
      { column: "title", value: value("text", title) },
    ]);
}

/** Writes the design straight to the session, then reopens so the config store loads it. */
async function seedDesign(
  user: User,
  file: string,
  build: (config: Config, formId: string) => void,
) {
  const config = await invoke<Config>("read_document_config", { windowLabel: "main" });
  build(config, config.design.forms[0].id);
  await invoke("update_document_config", { windowLabel: "main", config });
  const archive = join(process.env.IXTABLE_STATE_DIR ?? "", file);
  dialogMock.save.mockResolvedValueOnce(archive);
  await user.click(screen.getByRole("button", { name: "Save project" }));
  await screen.findByText("Saved archive", {}, LONG);
  await user.click(screen.getByRole("button", { name: "Close project" }));
  dialogMock.open.mockResolvedValueOnce(archive);
  await user.click(await screen.findByRole("button", { name: /Open document/i }, LONG));
  await screen.findByText("Saved archive", {}, LONG);
}

function taskForm(config: Config, formId: string, patch: Record<string, unknown>) {
  config.design.forms[0] = {
    ...config.design.forms[0],
    name: "Task",
    source: { kind: "table", table: "tasks" },
    layout: grid(),
    listColumns: ["title"],
    rules: [],
    controls: [titleField()],
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

async function openRuntime(user: User) {
  await user.click(screen.getByRole("button", { name: "Runtime" }));
  const page = await screen.findByRole("region", { name: "Application page" }, LONG);
  const nav = screen.getByRole("navigation", { name: "Application navigation" });
  await user.click(within(nav).getByRole("button", { name: "Tasks" }));
  return page;
}

const titles = async () =>
  (await readPage("tasks", [{ column: "id", descending: false }])).rows.map(
    (row: Array<{ value?: unknown }>) => row[1].value,
  );

it("edits, adds and deletes records in place on a continuous form", async () => {
  const user = await renderNewDocument();
  await seedTasks(["Draft plan", "Review budget"]);
  await seedDesign(user, "continuous.ixt", (config, formId) =>
    taskForm(config, formId, { modes: ["continuous", "create", "edit"] }),
  );
  const page = await openRuntime(user);
  const view = await within(page).findByRole("region", { name: "Task" }, LONG);
  const first = await within(view).findByRole("group", { name: "Record 1" }, LONG);
  expect(within(first).getByRole("textbox", { name: /Title/ })).toHaveValue("Draft plan");
  expect(within(view).getByRole("group", { name: "Record 2" })).toBeInTheDocument();

  // Edit in place: Save appears once the row changes.
  expect(within(first).queryByRole("button", { name: "Save" })).toBeNull();
  const title = within(first).getByRole("textbox", { name: /Title/ });
  await user.clear(title);
  await user.type(title, "Final plan");
  await user.click(within(first).getByRole("button", { name: "Save" }));
  await within(view).findByText("Changes saved.", {}, LONG);
  await waitFor(async () => expect(await titles()).toEqual(["Final plan", "Review budget"]), LONG);

  // Required fields validate per row.
  const fresh = within(view).getByRole("group", { name: "New record" });
  await user.type(within(fresh).getByRole("textbox", { name: /Title/ }), "x");
  await user.clear(within(fresh).getByRole("textbox", { name: /Title/ }));
  await user.type(within(fresh).getByRole("textbox", { name: /Title/ }), "Ship release");
  await user.click(within(fresh).getByRole("button", { name: "Add" }));
  await within(view).findByText("Record created.", {}, LONG);
  const third = await within(view).findByRole("group", { name: "Record 3" }, LONG);
  expect(within(third).getByRole("textbox", { name: /Title/ })).toHaveValue("Ship release");
  expect(
    within(within(view).getByRole("group", { name: "New record" })).getByRole("textbox", {
      name: /Title/,
    }),
  ).toHaveValue("");

  await user.click(within(third).getByRole("button", { name: "Delete" }));
  await user.click(await screen.findByRole("button", { name: "Confirm" }, LONG));
  await within(view).findByText("Record deleted.", {}, LONG);
  await waitFor(async () => expect(await titles()).toEqual(["Final plan", "Review budget"]), LONG);
});

it("shows the chosen row of a split form and steps through records with the navigation bar", async () => {
  const user = await renderNewDocument();
  await seedTasks(["Alpha", "Bravo", "Charlie"]);
  await seedDesign(user, "split.ixt", (config, formId) =>
    taskForm(config, formId, {
      modes: ["split", "detail", "create", "edit"],
      navigationBar: true,
    }),
  );
  const page = await openRuntime(user);
  await within(page).findByText("Choose a record to see it here.", {}, LONG);
  await user.click(await within(page).findByRole("row", { name: "Open Bravo" }, LONG));
  const detail = await within(page).findByRole("form", { name: "Task" }, LONG);
  expect(await within(detail).findByRole("textbox", { name: /Title/ }, LONG)).toHaveValue("Bravo");
  const bar = within(page).getByRole("group", { name: "Record navigation" });
  expect(await within(bar).findByText("Record 2 of 3", {}, LONG)).toBeInTheDocument();
  expect(within(page).getByRole("row", { name: "Open Bravo" })).toHaveAttribute(
    "aria-current",
    "true",
  );

  await user.click(within(bar).getByRole("button", { name: "Next record" }));
  await waitFor(
    () =>
      expect(
        within(within(page).getByRole("form", { name: "Task" })).getByRole("textbox", {
          name: /Title/,
        }),
      ).toHaveValue("Charlie"),
    LONG,
  );
  expect(within(bar).getByText("Record 3 of 3")).toBeInTheDocument();
  expect(within(bar).getByRole("button", { name: "Next record" })).toBeDisabled();
  expect(within(bar).getByRole("button", { name: "Last record" })).toBeDisabled();

  // Editing in the detail pane refreshes the datasheet above it.
  await user.click(within(page).getByRole("button", { name: "Edit" }));
  const edit = await within(page).findByRole("form", { name: "Edit Task" }, LONG);
  const field = await within(edit).findByRole("textbox", { name: /Title/ }, LONG);
  await user.clear(field);
  await user.type(field, "Charlie two");
  expect(within(bar).getByRole("button", { name: "First record" })).toBeDisabled();
  await user.click(within(edit).getByRole("button", { name: "Save" }));
  expect(await within(page).findByRole("row", { name: "Open Charlie two" }, LONG)).toBeVisible();

  await user.click(within(bar).getByRole("button", { name: "First record" }));
  await waitFor(
    () =>
      expect(
        within(within(page).getByRole("form", { name: "Task" })).getByRole("textbox", {
          name: /Title/,
        }),
      ).toHaveValue("Alpha"),
    LONG,
  );
  expect(within(bar).getByText("Record 1 of 3")).toBeInTheDocument();
});

it("follows the list's sort order when stepping from an opened record", async () => {
  const user = await renderNewDocument();
  await seedTasks(["Alpha", "Bravo", "Charlie"]);
  await seedDesign(user, "navbar.ixt", (config, formId) =>
    taskForm(config, formId, { modes: ["list", "detail", "create", "edit"], navigationBar: true }),
  );
  const page = await openRuntime(user);
  const list = await within(page).findByRole("region", { name: "Task" }, LONG);
  await within(list).findByRole("row", { name: "Open Alpha" }, LONG);
  await user.click(within(list).getByRole("button", { name: "Title" }));
  await user.click(within(list).getByRole("button", { name: "Title" }));
  await waitFor(() =>
    expect(within(list).getAllByRole("row")[1]).toHaveAccessibleName("Open Charlie"),
  );
  await user.click(within(list).getByRole("row", { name: "Open Charlie" }));
  const bar = await within(page).findByRole("group", { name: "Record navigation" }, LONG);
  expect(await within(bar).findByText("Record 1 of 3", {}, LONG)).toBeInTheDocument();
  await user.click(within(bar).getByRole("button", { name: "Next record" }));
  await waitFor(
    () =>
      expect(
        within(within(page).getByRole("form", { name: "Task" })).getByRole("textbox", {
          name: /Title/,
        }),
      ).toHaveValue("Bravo"),
    LONG,
  );
  await user.click(within(bar).getByRole("button", { name: "New record" }));
  expect(await within(page).findByRole("form", { name: "New Task" }, LONG)).toBeInTheDocument();
});

it("opens a popup form from an action and uses what it returns", async () => {
  const user = await renderNewDocument();
  await seedTasks(["Alpha"]);
  const addAction = id();
  const pickAction = id();
  const launcher = id();
  await seedDesign(user, "popup.ixt", (config, formId) => {
    taskForm(config, formId, {
      modes: ["detail", "create", "edit"],
      controls: [
        titleField(),
        {
          id: id(),
          kind: "button",
          label: "Use this task",
          validation: { required: false },
          placement: at(2),
          actionId: pickAction,
        },
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
          kind: "button",
          label: "Add a task",
          validation: { required: false },
          placement: at(1),
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
          {
            id: id(),
            kind: "openForm",
            formId,
            mode: "create",
            popup: true,
            storeAs: "task",
          },
          { id: id(), kind: "message", text: "'Added ' & results.task.title" },
          {
            id: id(),
            kind: "openForm",
            formId,
            mode: "detail",
            recordId: "results.task.id",
            popup: true,
            storeAs: "picked",
          },
          { id: id(), kind: "message", text: "'Picked ' & results.picked" },
        ],
      },
      {
        id: pickAction,
        name: "Use this task",
        onError: "stop",
        steps: [{ id: id(), kind: "closeForm", value: "upper(record.title)" }],
      },
    ];
  });
  const page = await openRuntime(user);
  const planner = await within(page).findByRole("form", { name: "New Planner" }, LONG);
  await user.click(within(planner).getByRole("button", { name: "Add a task" }));

  const create = await screen.findByRole("dialog", { name: "Task" }, LONG);
  await user.type(within(create).getByRole("textbox", { name: /Title/ }), "Write docs");
  await user.click(within(create).getByRole("button", { name: "Create" }));

  // The saved record comes back; the next step opens it in a second popup.
  await waitFor(() => expect(create).not.toBeInTheDocument(), LONG);
  const detail = await screen.findByRole("dialog", { name: "Task" }, LONG);
  await waitFor(
    () => expect(within(detail).getByRole("textbox", { name: /Title/ })).toHaveValue("Write docs"),
    LONG,
  );
  await user.click(await within(detail).findByRole("button", { name: "Use this task" }, LONG));
  await within(planner).findByText("Picked WRITE DOCS", {}, LONG);
  expect(screen.queryByRole("dialog", { name: "Task" })).toBeNull();
  expect(await titles()).toEqual(["Alpha", "Write docs"]);
});

it("returns null and keeps the opener usable when a popup is dismissed", async () => {
  const user = await renderNewDocument();
  await seedTasks(["Alpha"]);
  const addAction = id();
  await seedDesign(user, "popup-dismiss.ixt", (config, formId) => {
    taskForm(config, formId, { modes: ["create"] });
    const launcher = id();
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
          kind: "button",
          label: "Add a task",
          validation: { required: false },
          placement: at(1),
          actionId: addAction,
        },
      ],
    });
    config.design.navigation[0] = {
      ...config.design.navigation[0],
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
            text: "if(isnull(results.task), 'Nothing added', 'Added')",
          },
        ],
      },
    ];
  });
  const page = await openRuntime(user);
  const planner = await within(page).findByRole("form", { name: "New Planner" }, LONG);
  await user.click(within(planner).getByRole("button", { name: "Add a task" }));
  const popup = await screen.findByRole("dialog", { name: "Task" }, LONG);
  await user.click(within(popup).getByRole("button", { name: "Close Task" }));
  await within(planner).findByText("Nothing added", {}, LONG);
  expect(screen.queryByRole("dialog")).toBeNull();
});
