import { open, save } from "@tauri-apps/plugin-dialog";

export const chooseAccessFile = () =>
  open({
    title: "Import Access database",
    multiple: false,
    directory: false,
    filters: [{ name: "Access databases and templates", extensions: ["accdb", "mdb", "accdt"] }],
  });

export const chooseReportDestination = (name: string, format: "md" | "csv") =>
  save({
    title: "Export migration report",
    defaultPath: `${name}.${format}`,
    filters: [
      format === "md"
        ? { name: "Markdown", extensions: ["md"] }
        : { name: "CSV", extensions: ["csv"] },
    ],
  });
