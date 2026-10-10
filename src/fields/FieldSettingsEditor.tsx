import { useState } from "react";
import { asTauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import type { DbColumn } from "../lib/types";
import { newId } from "../lib/utils";
import { logicalOf, parseLogical } from "../schema/logical";
import type { EntitySettings } from "../schema/types";
import { removeUnusedRecordAttachments } from "./api";
import { MaskField } from "./MaskField";
import { FIELD_FORMATS, type FieldSettings } from "./types";

const blank = (field: FieldSettings) =>
  !field.format && !field.inputMask && !(field.options ?? []).length;

/**
 * Field settings of one table: rich text, attachments, multiple choices, and input
 * masks. They are definition edits that apply at once; they never change the column.
 */
export function FieldSettingsEditor({ table, columns }: { table: string; columns: DbColumn[] }) {
  const { config, update } = useDocumentConfig();
  const [notice, setNotice] = useState("");
  const entity = config.entities.find((e) => e.table === table);
  const fieldFor = (column: string) => entity?.fields?.find((f) => f.column === column);
  const save = (column: string, patch: Partial<FieldSettings>) =>
    update((draft) => {
      const existing = draft.entities.find((e) => e.table === table);
      const before = existing?.fields?.find((f) => f.column === column);
      const next: FieldSettings = { id: before?.id ?? newId(), column, ...before, ...patch };
      const others = (existing?.fields ?? []).filter((f) => f.column !== column);
      const fields = blank(next) ? others : [...others, next];
      const settings: EntitySettings = {
        id: existing?.id ?? newId(),
        table,
        concurrency: "optimistic",
        ...existing,
        fields,
      };
      return {
        ...draft,
        entities: existing
          ? draft.entities.map((e) => (e.table === table ? settings : e))
          : [...draft.entities, settings],
      };
    }, `Field settings for ${table}.${column}`);
  const editable = columns.filter(
    (c) => !c.generated && !c.primaryKeyPosition && ["text", "json"].includes(logicalBase(c)),
  );
  const hasAttachments = (entity?.fields ?? []).some((f) => f.format === "attachment");
  const cleanup = async () => {
    try {
      const removed = await removeUnusedRecordAttachments();
      setNotice(removed === 1 ? "Removed 1 unused file." : `Removed ${removed} unused files.`);
    } catch (reason) {
      setNotice(asTauriError(reason).message);
    }
  };
  return (
    <section aria-label="Field settings">
      <h3>Field settings</h3>
      <p className="text-slate-600">
        How text and JSON columns are entered and shown in forms, the datasheet, and reports. These
        apply at once and do not change the column.
      </p>
      {editable.length === 0 && <p>This table has no text or JSON columns.</p>}
      {editable.map((column) => {
        const field = fieldFor(column.name);
        const base = logicalBase(column);
        return (
          <div
            className="flex flex-wrap items-start gap-3 border-b border-slate-100 py-2"
            key={column.name}
          >
            <b className="min-w-28">{column.name}</b>
            <label className="grid gap-1">
              Format
              <select
                aria-label={`${column.name} format`}
                value={field?.format ?? ""}
                onChange={(e) =>
                  save(column.name, {
                    format: e.target.value || null,
                    ...(e.target.value ? { inputMask: null } : {}),
                  })
                }
              >
                <option value="">Plain</option>
                {FIELD_FORMATS.filter((f) => f.types.includes(base)).map((f) => (
                  <option key={f.value} value={f.value}>
                    {f.label}
                  </option>
                ))}
              </select>
            </label>
            {!field?.format && base === "text" && (
              <MaskField
                label={`${column.name} input mask`}
                value={field?.inputMask}
                onChange={(inputMask) => save(column.name, { inputMask })}
              />
            )}
            {field?.format === "multiSelect" && (
              <ChoicesField
                column={column.name}
                options={field.options ?? []}
                onChange={(options) => save(column.name, { options })}
              />
            )}
          </div>
        );
      })}
      {hasAttachments && (
        <div className="flex flex-wrap items-center gap-3 py-2">
          <button onClick={cleanup}>Remove unused files</button>
          <span className="text-slate-600" role="status">
            {notice ||
              "Deletes stored files that no record refers to and that are over an hour old."}
          </span>
        </div>
      )}
    </section>
  );
}

const logicalBase = (column: DbColumn) => parseLogical(logicalOf(column)).base;

/** Choices typed one per line, saved when the box loses focus. */
function ChoicesField({
  column,
  options,
  onChange,
}: {
  column: string;
  options: string[];
  onChange: (options: string[]) => void;
}) {
  const [text, setText] = useState(options.join("\n"));
  return (
    <label className="grid gap-1">
      Choices (one per line)
      <textarea
        aria-label={`${column} choices`}
        rows={3}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          const next = [
            ...new Set(
              text
                .split("\n")
                .map((line) => line.trim())
                .filter(Boolean),
            ),
          ];
          if (next.join("\n") !== options.join("\n")) onChange(next);
        }}
      />
    </label>
  );
}
