import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { ExportMenu } from "../../src/export/ExportMenu";

const save = vi.fn<(options: Record<string, unknown>) => Promise<string | null>>();
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (o: Record<string, unknown>) => save(o) }));

let user: ReturnType<typeof userEvent.setup>;
beforeEach(() => {
  save.mockReset();
  user = userEvent.setup();
});

const openMenu = async () => {
  await user.click(screen.getByRole("button", { name: "Export" }));
  return screen.findByRole("menu");
};

it("lists the three formats and closes on Escape", async () => {
  render(<ExportMenu name="orders" onExport={vi.fn()} />);
  const menu = await openMenu();
  expect(menu).toBeInTheDocument();
  expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
    "CSV",
    "Excel (.xlsx)",
    "JSON",
  ]);
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

it("does nothing when the save dialog is cancelled", async () => {
  const onExport = vi.fn();
  save.mockResolvedValue(null);
  render(<ExportMenu name="orders" onExport={onExport} />);
  await openMenu();
  await user.click(screen.getByRole("menuitem", { name: "CSV" }));
  expect(save).toHaveBeenCalledOnce();
  expect(onExport).not.toHaveBeenCalled();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("exports to the chosen path with the extension ensured, busy then status", async () => {
  let finish: (summary: { rows: number; path: string }) => void = () => undefined;
  const onExport = vi.fn(() => new Promise<{ rows: number; path: string }>((r) => (finish = r)));
  save.mockResolvedValue("/tmp/out");
  render(<ExportMenu name="Sales/Q1" onExport={onExport} />);
  await openMenu();
  await user.click(screen.getByRole("menuitem", { name: "Excel (.xlsx)" }));
  expect(save.mock.calls[0][0]).toMatchObject({
    defaultPath: "SalesQ1.xlsx",
    filters: [{ name: "Excel workbooks", extensions: ["xlsx"] }],
  });
  expect(onExport).toHaveBeenCalledWith("xlsx", "/tmp/out.xlsx");
  const button = screen.getByRole("button", { name: "Export" });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("aria-busy", "true");
  finish({ rows: 1234, path: "/tmp/out.xlsx" });
  expect(await screen.findByRole("status")).toHaveTextContent("Exported 1,234 rows");
  expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
});

it("shows the error from a failed export", async () => {
  save.mockResolvedValue("/tmp/out.json");
  render(<ExportMenu name="orders" onExport={() => Promise.reject(new Error("disk full"))} />);
  await openMenu();
  await user.click(screen.getByRole("menuitem", { name: "JSON" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
