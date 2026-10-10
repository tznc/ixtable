import { call, saveDocumentAs } from "../lib/api";
import type { SessionState } from "../lib/types";
import { chooseDocumentDestination } from "../lib/dialog";
import type {
  ArchiveSizeReport,
  AssetImport,
  Attachment,
  CheckpointInfo,
  LogEntry,
  OrphanCleanup,
  RecoveryRecord,
} from "./types";

/** Saves only when dirty, titled, and not conflicted; otherwise returns the current state. */
export const autosaveDocument = () => call<SessionState>("autosave_document");
export const documentState = () => call<SessionState>("document_state");

export const listRecoverableSessions = () => call<RecoveryRecord[]>("list_recovery_sessions");
export const discardRecovery = (sessionId: string) => call<void>("discard_recovery", { sessionId });
export const recoverSession = (sessionId: string) =>
  call<SessionState>("recover_session", { sessionId });

/** Recovers crashed WIP; untitled work asks for a destination (canceling keeps it open, unsaved). */
export async function recoverWork(record: RecoveryRecord): Promise<SessionState> {
  const state = await recoverSession(record.sessionId);
  // The saved file could not be safely replaced (unreadable, or no safety checkpoint): ask where to save instead.
  const needsSaveAs = state.lastError?.code === "RECOVERY_NEEDS_SAVE_AS";
  if (state.path && !needsSaveAs) return state;
  const path = await chooseDocumentDestination(record.name || "Recovered");
  return path ? saveDocumentAs(path) : state;
}

export const createCheckpoint = (reason = "manual") =>
  call<CheckpointInfo>("create_checkpoint", { reason });
export const listCheckpoints = () => call<CheckpointInfo[]>("list_checkpoints");
export const restoreCheckpointAsCopy = (checkpointId: string, path: string) =>
  call<string>("restore_checkpoint_as_copy", { checkpointId, path });

export const listAssets = () => call<Attachment[]>("list_attachments");
export const importAsset = (path: string, mediaType?: string) =>
  call<AssetImport>("import_asset", { path, mediaType: mediaType ?? null });
export const exportAsset = (id: string, path: string) =>
  call<void>("export_attachment", { id, path });
export const removeAsset = (id: string) => call<SessionState>("remove_attachment", { id });
export const listOrphanAssets = () => call<Attachment[]>("list_orphan_assets");
export const cleanupOrphanAssets = () => call<OrphanCleanup>("cleanup_orphan_assets");
export const archiveSizeReport = () => call<ArchiveSizeReport>("archive_size_report");

export const readLogs = (limit = 200) => call<LogEntry[]>("read_logs", { limit });
/** Appends a line to the local diagnostic log (redacted in Rust). */
export const writeLog = (level: "info" | "warn" | "error", area: string, message: string) =>
  call<void>("write_log", { level, area, message });
/** A text asset's content for the Studio preview (`text/*`, at most 2 MB). */
export const readAssetText = (id: string) => call<string>("read_asset_text", { id });
