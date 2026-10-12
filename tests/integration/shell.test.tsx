import { join } from "node:path";
import { screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { registerRecordHook } from "../../src/lib/records";
import { readPage, renderNewDocument } from "./helpers";
import { dialogMock } from "./setup";

const LONG = { timeout: 20_000 };
type Config = { name: string; activeMode: string; savedQueries: Array<{ name: string }> };
const readConfig = () => invoke<Config>("read_document_config", { windowLabel: "main" });

async function openSettingsTab(user: Awaited<ReturnType<typeof renderNewDocument>>, tab: string) {
  await user.click(screen.getByRole("button", { name: "Settings" }));
  await screen.findByRole("heading", { name: "Application settings" }, LONG);
  await user.click(screen.getByRole("tab", { name: tab }));
}

it("creates a table from the object browser and routes grid writes through record hooks", async () => {
  const user = await renderNewDocument();
  const writes: string[] = [];
  const unregister = registerRecordHook({
    before: (write) => {
      writes.push(`before ${write.operation} ${write.table}`);
    },
    after: (write) => {
      writes.push(`after ${write.operation} ${write.table}`);
    },
  });
  await user.click(screen.getByRole("button", { name: "New table" }));
  const form = await screen.findByRole("form", { name: "Create table" });
  await user.type(within(form).getByLabelText("Table name"), "tasks");
  await user.clear(within(form).getByRole("textbox", { name: "Column 2 name" }));
  await user.type(within(form).getByRole("textbox", { name: "Column 2 name" }), "title");
  await user.click(within(form).getByRole("button", { name: "Create table" }));

  const tasks = await screen.findByRole("button", { name: /^tasks\b/ }, LONG);
  await waitFor(() => expect(tasks).toHaveAttribute("aria-pressed", "true"), LONG);
  const schema = await invoke<{ columns: Array<{ name: string; primaryKeyPosition: number }> }>(
    "inspect_table",
    { windowLabel: "main", table: "tasks" },
  );
  expect(schema.columns.map((column) => [column.name, column.primaryKeyPosition])).toEqual([
    ["id", 1],
    ["title", 0],
  ]);
  await user.type(await screen.findByRole("textbox", { name: "New id" }, LONG), "1");
  await user.type(screen.getByRole("textbox", { name: "New title" }), "Write tests{Enter}");
  await waitFor(() => expect(writes).toEqual(["before insert tasks", "after insert tasks"]));
  expect((await readPage("tasks")).total).toBe(1);
  unregister();
});

it("lists recent documents on the start screen and opens one with a click", async () => {
  const user = await renderNewDocument();
  const path = join(process.env.IXTABLE_STATE_DIR!, "recent-pick.ixt");
  dialogMock.save.mockResolvedValueOnce(path);
  await user.click(screen.getByRole("button", { name: "Save project" }));
  await screen.findByText("Saved archive", {}, LONG);
  await user.click(screen.getByRole("button", { name: "Close project" }));

  const recent = await screen.findByRole("button", { name: "Open recent recent-pick.ixt" }, LONG);
  await user.click(recent);
  expect(await screen.findByText("Saved archive", {}, LONG)).toBeInTheDocument();
  const state = await invoke<{ path: string }>("document_state", { windowLabel: "main" });
  expect(state.path).toBe(path);
});

it("switches between every registered mode and persists the active mode", async () => {
  const user = await renderNewDocument();
  const headings: Array<[string, string]> = [
    ["Query", "Query"],
    ["Reports", "Reports"],
    ["Dashboards", "Dashboards"],
    ["Automation", "Automation"],
    ["Runtime", "Runtime"],
    ["Settings", "Application settings"],
    ["Design", "Form builder"],
  ];
  for (const [mode, heading] of headings) {
    await user.click(screen.getByRole("button", { name: mode }));
    expect(await screen.findByRole("heading", { name: heading }, LONG)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: mode })).toHaveAttribute("aria-pressed", "true");
  }
  expect((await readConfig()).activeMode).toBe("design");
  await user.click(screen.getByRole("button", { name: "Preview" }));
  expect(await screen.findByRole("region", { name: "Main form preview" })).toBeInTheDocument();
});

it("edits config YAML, reports YAML errors, and undoes an applied YAML change", async () => {
  const user = await renderNewDocument();
  await openSettingsTab(user, "YAML");
  const editor = await screen.findByRole("textbox", { name: "Configuration YAML" });
  await waitFor(() => expect(editor).toHaveDisplayValue(/name: Untitled/));

  await user.clear(editor);
  await user.click(editor);
  await user.paste("name: [unclosed");
  await user.click(screen.getByRole("button", { name: "Apply YAML" }));
  expect(await screen.findByRole("alert", {}, LONG)).toHaveTextContent("INVALID_CONFIG");

  await user.clear(editor);
  await user.click(editor);
  await user.paste("name: Inventory\nactiveMode: app\nversion: 2\n");
  await user.click(screen.getByRole("button", { name: "Apply YAML" }));
  expect(await screen.findByText("YAML applied.", {}, LONG)).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(await screen.findByText("Inventory")).toBeInTheDocument();
  await waitFor(() => expect(editor).toHaveDisplayValue(/version: 5/));

  await user.click(screen.getByRole("button", { name: "Undo" }));
  await waitFor(async () => expect((await readConfig()).name).toBe("Untitled"), LONG);
  await waitFor(() => expect(editor).toHaveDisplayValue(/name: Untitled/));
});

it("lists validation problems from validate_document", async () => {
  const user = await renderNewDocument();
  await openSettingsTab(user, "Problems");
  expect(await screen.findByText("No problems found.", {}, LONG)).toBeInTheDocument();

  await user.click(screen.getByRole("tab", { name: "YAML" }));
  const editor = await screen.findByRole("textbox", { name: "Configuration YAML" });
  await waitFor(() => expect(editor).toHaveDisplayValue(/name: Untitled/));
  await user.clear(editor);
  await user.click(editor);
  await user.paste(
    "name: Broken\nactiveMode: app\nversion: 3\nreports:\n  - id: r1\n    name: Sales\n  - id: r1\n    name: ''\n",
  );
  await user.click(screen.getByRole("button", { name: "Apply YAML" }));
  await screen.findByText("YAML applied.", {}, LONG);

  await user.click(screen.getByRole("tab", { name: "Problems" }));
  const list = await screen.findByRole("list", { name: "Problems" }, LONG);
  expect(within(list).getByText(/duplicate report id r1/)).toBeInTheDocument();
  expect(within(list).getByText(/report r1 has no name/)).toBeInTheDocument();
  expect(await screen.findByText("1 error, 1 warning")).toBeInTheDocument();
});

it("undoes and redoes saved-query edits from the toolbar and keyboard", async () => {
  const user = await renderNewDocument();
  await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument(), LONG);
  expect(screen.getByRole("button", { name: "Undo" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "New query" }));
  const name = await screen.findByRole("textbox", { name: "Query name" });
  await user.clear(name);
  await user.type(name, "Answer");
  await user.click(screen.getByRole("button", { name: "Save query" }));
  expect(await screen.findByRole("button", { name: "Answer" }, LONG)).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Undo" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Answer" })).not.toBeInTheDocument(),
  );
  await waitFor(async () => expect((await readConfig()).savedQueries).toEqual([]), LONG);

  await user.click(screen.getByRole("button", { name: "Redo" }));
  expect(await screen.findByRole("button", { name: "Answer" }, LONG)).toBeInTheDocument();
  await user.keyboard("{Control>}z{/Control}");
  await waitFor(async () => expect((await readConfig()).savedQueries).toEqual([]), LONG);
  await user.keyboard("{Control>}{Shift>}z{/Shift}{/Control}");
  await waitFor(
    async () =>
      expect((await readConfig()).savedQueries).toEqual([
        expect.objectContaining({ name: "Answer" }),
      ]),
    LONG,
  );
});

it("keeps the mode out of undo history", async () => {
  const user = await renderNewDocument();
  await user.click(screen.getByRole("button", { name: "Reports" }));
  await screen.findByRole("heading", { name: "Reports" }, LONG);
  expect(screen.getByRole("button", { name: "Undo" })).toBeDisabled();
  vi.spyOn(window, "confirm").mockReturnValue(true);
  await user.click(screen.getByRole("button", { name: "Close project" }));
  await screen.findByRole("heading", { name: "Your data, in one portable file." }, LONG);
});
