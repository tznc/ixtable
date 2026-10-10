import { Copy, Pencil, Plus, Trash2, Wand2 } from "lucide-react";
import { useState } from "react";
import { inspectTable } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import type { DbObject, TableSchema } from "../lib/types";
import type { EntitySettings } from "../schema/types";
import { generateCrudForms } from "./generate";
import { useGenerateApp } from "./generateApp";
import { deleteForm, duplicateForm } from "./operations";
import { newForm } from "./schema";
import { useDesignEditor } from "./useDesignEditor";

/** Builds list + detail forms for a table, with lookups and related lists from the live schema. */
async function generateFor(table: string, objects: DbObject[], entities: EntitySettings[]) {
  const schema = await inspectTable(table);
  const others: TableSchema[] = [];
  for (const object of objects) {
    if (object.objectType !== "table" || object.name === table) continue;
    const other = await inspectTable(object.name).catch(() => null);
    if (other) others.push(other);
  }
  const known = others;
  const targets = Object.fromEntries(known.map((t) => [t.name, t]));
  const children = known.filter((t) => t.foreignKeys.some((fk) => fk.targetTable === table));
  return generateCrudForms(schema, { targets, children, entities });
}

/** Form list: select, new, rename, duplicate, delete, and "Generate form from table". */
export function FormList({
  selectedId,
  onSelect,
  objects,
}: {
  selectedId?: string;
  onSelect: (id: string) => void;
  objects: DbObject[];
}) {
  const { design, editDesign, editForm } = useDesignEditor();
  const { config } = useDocumentConfig();
  const [renaming, setRenaming] = useState<string | null>(null);
  const [table, setTable] = useState("");
  const [error, setError] = useState("");
  const tables = objects.filter((o) => o.objectType === "table");
  const forms = design.forms;
  const generateApp = useGenerateApp();
  const [notice, setNotice] = useState("");

  const create = () => {
    const form = newForm(`Form ${forms.length + 1}`);
    onSelect(form.id);
    editDesign((d) => ({ ...d, forms: [...d.forms, form] }), "New form");
  };
  const duplicate = (id: string) => {
    const source = forms.find((f) => f.id === id);
    if (!source) return;
    const copy = duplicateForm(source);
    onSelect(copy.id);
    editDesign((d) => ({ ...d, forms: [...d.forms, copy] }), "Duplicate form");
  };
  const remove = (id: string) => {
    const next = forms.find((f) => f.id !== id);
    if (next) onSelect(next.id);
    editDesign((d) => deleteForm(d, id), "Delete form");
  };
  const generate = async () => {
    const name = table || tables[0]?.name;
    if (!name) return;
    try {
      const { list, detail, navigation } = await generateFor(name, objects, config.entities);
      setError("");
      onSelect(detail.id);
      editDesign(
        (d) => ({
          ...d,
          forms: [...d.forms, list, detail],
          navigation: [...d.navigation, navigation],
          startPage: d.startPage ?? navigation.id,
        }),
        "Generate form",
      );
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <div className="fd-forms" role="region" aria-label="Forms">
      <small>FORMS</small>
      <ul>
        {forms.map((form) => (
          <li key={form.id} className={form.id === selectedId ? "active" : ""}>
            {renaming === form.id ? (
              <input
                aria-label={`Rename ${form.name}`}
                autoFocus
                defaultValue={form.name}
                onBlur={(e) => {
                  setRenaming(null);
                  const name = e.target.value.trim();
                  if (name && name !== form.name)
                    editForm(form.id, (f) => ({ ...f, name }), "Rename form");
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") e.currentTarget.blur();
                  if (e.key === "Escape") setRenaming(null);
                }}
              />
            ) : (
              <button
                type="button"
                className="fd-form-name"
                aria-pressed={form.id === selectedId}
                onClick={() => onSelect(form.id)}
              >
                {form.name || "Untitled form"}
              </button>
            )}
            <span className="fd-form-tools">
              <button
                type="button"
                aria-label={`Rename form ${form.name}`}
                title="Rename"
                onClick={() => setRenaming(form.id)}
              >
                <Pencil aria-hidden="true" />
              </button>
              <button
                type="button"
                aria-label={`Duplicate form ${form.name}`}
                title="Duplicate"
                onClick={() => duplicate(form.id)}
              >
                <Copy aria-hidden="true" />
              </button>
              <button
                type="button"
                aria-label={`Delete form ${form.name}`}
                title="Delete"
                onClick={() => remove(form.id)}
              >
                <Trash2 aria-hidden="true" />
              </button>
            </span>
          </li>
        ))}
      </ul>
      <button type="button" onClick={create}>
        <Plus aria-hidden="true" />
        New form
      </button>
      <div className="fd-generate">
        <label>
          Table to generate from
          <select value={table || tables[0]?.name || ""} onChange={(e) => setTable(e.target.value)}>
            {tables.map((t) => (
              <option key={t.name}>{t.name}</option>
            ))}
          </select>
        </label>
        <button
          type="button"
          disabled={!tables.length}
          onClick={() => generate().catch(() => undefined)}
        >
          <Wand2 aria-hidden="true" />
          Generate form from table
        </button>
        <button
          type="button"
          disabled={!tables.length}
          onClick={() =>
            generateApp()
              .then((added) => {
                setError("");
                setNotice(
                  added ? `Added ${added} forms and pages.` : "Every table already has forms.",
                );
              })
              .catch((reason) =>
                setError(reason instanceof Error ? reason.message : String(reason)),
              )
          }
        >
          <Wand2 aria-hidden="true" />
          Generate app from tables
        </button>
        {notice && <small role="status">{notice}</small>}
        {error && (
          <small className="fd-problem" role="alert">
            {error}
          </small>
        )}
      </div>
    </div>
  );
}
