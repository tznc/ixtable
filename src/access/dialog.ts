import { open } from "@tauri-apps/plugin-dialog";

export const chooseAccessFile = () =>
  open({
    title: "Import Access database",
    multiple: false,
    directory: false,
    filters: [{ name: "Access databases and templates", extensions: ["accdb", "mdb", "accdt"] }],
  });
