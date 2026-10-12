import { call } from "../lib/api";
import type { AttachmentRef } from "./types";

/** Largest file one attachment can hold (decimal megabytes), as Rust enforces it. */
export const MAX_ATTACHMENT_BYTES = 20_000_000;

export const uploadRecordAttachment = (args: {
  table: string;
  column: string;
  name: string;
  mime: string | null;
  contentBase64: string;
}) => call<AttachmentRef>("upload_record_attachment", args);

export const saveRecordAttachment = (id: string, path: string) =>
  call<void>("save_record_attachment", { id, path });

/** Deletes stored files no record refers to (Studio only); returns how many. */
export const removeUnusedRecordAttachments = () => call<number>("remove_unused_record_attachments");
