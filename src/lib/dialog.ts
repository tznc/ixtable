import { open, save } from "@tauri-apps/plugin-dialog";

const ixtFilter = { name: "ixtable documents", extensions: ["ixt"] };

export const chooseDocumentToOpen = () =>
  open({
    title: "Open ixtable document",
    multiple: false,
    directory: false,
    filters: [ixtFilter],
  });

export const chooseDocumentDestination = (name: string) =>
  save({
    title: "Save ixtable document",
    defaultPath: name.toLowerCase().endsWith(".ixt") ? name : `${name}.ixt`,
    filters: [ixtFilter],
  });

const bundleFilter = { name: "ixtable runtime bundles", extensions: ["ixtr"] };

export const chooseBundleToOpen = () =>
  open({
    title: "Open ixtable runtime bundle",
    multiple: false,
    directory: false,
    filters: [bundleFilter],
  });

export const chooseBundleDestination = (name: string) =>
  save({
    title: "Export runtime bundle",
    defaultPath: name.toLowerCase().endsWith(".ixtr") ? name : `${name}.ixtr`,
    filters: [bundleFilter],
  });

const pdfFilter = { name: "PDF documents", extensions: ["pdf"] };

/** Save dialog for an exported PDF (Reports: Export PDF…). */
export const choosePdfDestination = (name: string) =>
  save({
    title: "Export PDF",
    defaultPath: name.toLowerCase().endsWith(".pdf") ? name : `${name}.pdf`,
    filters: [pdfFilter],
  });

/** Save dialog for one file of an attachment field. */
export const chooseAttachmentDestination = (name: string) =>
  save({ title: "Save attachment", defaultPath: name });
