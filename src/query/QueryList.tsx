import { Check, Copy, FileCode2, Pencil, Plus, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { SavedQuery } from "./types";

/** Saved queries with new, rename, duplicate, and delete. */
export function QueryList({
  queries,
  activeId,
  draftLabel,
  onOpen,
  onNew,
  onRename,
  onDuplicate,
  onDelete,
}: {
  queries: SavedQuery[];
  activeId: string | null;
  /** Shown for an unsaved new query. */
  draftLabel: string | null;
  onOpen: (id: string) => void;
  onNew: () => void;
  onRename: (id: string, name: string) => void;
  onDuplicate: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  return (
    <nav className="query-list" aria-label="Saved queries">
      <div className="object-group-heading">
        <FileCode2 />
        <h3>Queries</h3>
        <em>{queries.length}</em>
        <button aria-label="New query" onClick={onNew}>
          <Plus />
        </button>
      </div>
      <ul>
        {queries.map((q) => (
          <li key={q.id}>
            {renaming?.id === q.id ? (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  if (renaming.name.trim()) onRename(q.id, renaming.name.trim());
                  setRenaming(null);
                }}
              >
                <input
                  aria-label={`New name for ${q.name}`}
                  value={renaming.name}
                  // biome-ignore lint/a11y/noAutofocus: focus moves into the inline rename field the user opened
                  autoFocus
                  onChange={(e) => setRenaming({ id: q.id, name: e.target.value })}
                  onKeyDown={(e) => e.key === "Escape" && setRenaming(null)}
                />
                <button type="submit" aria-label="Confirm rename">
                  <Check />
                </button>
                <button type="button" aria-label="Cancel rename" onClick={() => setRenaming(null)}>
                  <X />
                </button>
              </form>
            ) : (
              <>
                <button
                  className={activeId === q.id ? "selected" : ""}
                  aria-pressed={activeId === q.id}
                  onClick={() => onOpen(q.id)}
                >
                  <FileCode2 />
                  <span>{q.name || "Untitled query"}</span>
                  {q.action && <small className="query-action-tag">action</small>}
                </button>
                <span className="query-list-actions">
                  <button
                    aria-label={`Rename ${q.name}`}
                    onClick={() => setRenaming({ id: q.id, name: q.name })}
                  >
                    <Pencil />
                  </button>
                  <button aria-label={`Duplicate ${q.name}`} onClick={() => onDuplicate(q.id)}>
                    <Copy />
                  </button>
                  <button aria-label={`Delete ${q.name}`} onClick={() => onDelete(q.id)}>
                    <Trash2 />
                  </button>
                </span>
              </>
            )}
          </li>
        ))}
        {draftLabel && (
          <li>
            <button className="selected" aria-pressed="true">
              <FileCode2 />
              <span>
                {draftLabel} <small>Unsaved</small>
              </span>
            </button>
          </li>
        )}
      </ul>
      {!queries.length && !draftLabel && (
        <small className="object-empty">No saved queries yet</small>
      )}
    </nav>
  );
}
