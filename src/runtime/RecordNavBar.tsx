import { ChevronFirst, ChevronLast, ChevronLeft, ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import type { DesignForm } from "../design/schema";
import { type RecordCursor, recordAt } from "./cursor";
import { useRuntimeNavigation } from "./navigation";

type Props = {
  /** The form whose records the bar moves through (the list form). */
  form: DesignForm;
  /** Position of the shown record; null when it was opened without a list (direct link). */
  cursor: RecordCursor | null;
  onGo: (recordId: unknown, cursor: RecordCursor) => void;
  /** Opens a new record; absent when the role or form cannot create. */
  onNew?: () => void;
  /** Blocks moving away (unsaved edits, a running save). */
  disabled?: boolean;
};

/**
 * Access-style record navigation: first, previous, next, last and new, with the shown
 * record's position. Positions are read through the list's own page reader (`recordAt`),
 * so the order is the list's sort and search.
 */
export function RecordNavBar({ form, cursor, onGo, onNew, disabled = false }: Props) {
  const { app } = useRuntimeNavigation();
  const [total, setTotal] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const sorts = cursor?.sorts ?? [];
  const filters = cursor?.filters ?? [];
  const params = cursor?.params;
  const order = JSON.stringify([form.id, sorts, filters, params ?? null]);

  useEffect(() => {
    let live = true;
    recordAt(form, { sorts, filters, params }, 0, app)
      .then((found) => live && setTotal(found?.total ?? 0))
      .catch(() => live && setTotal(null));
    return () => {
      live = false;
    };
    // `order` covers the form, sort, search and parameters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [order, app]);

  const go = (index: number) => {
    if (busy || total == null) return;
    setBusy(true);
    setError("");
    recordAt(form, { sorts, filters, params }, index, app)
      .then((found) => {
        if (found?.recordId == null) return;
        onGo(found.recordId, { formId: form.id, index, sorts, filters, params });
      })
      .catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)))
      .finally(() => setBusy(false));
  };
  const index = cursor?.index ?? null;
  const last = total == null ? -1 : total - 1;
  const blocked = disabled || busy || total == null;
  const position =
    total == null
      ? "Loading…"
      : index == null
        ? `${total} records`
        : `Record ${Math.min(index + 1, total)} of ${total}`;
  return (
    <div className="rt-recnav" role="group" aria-label="Record navigation">
      <button
        type="button"
        aria-label="First record"
        title="First record"
        disabled={blocked || !total || index === 0}
        onClick={() => go(0)}
      >
        <ChevronFirst aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Previous record"
        title="Previous record"
        disabled={blocked || index == null || index <= 0}
        onClick={() => index != null && go(index - 1)}
      >
        <ChevronLeft aria-hidden="true" />
      </button>
      <span className="rt-recnav-position">{position}</span>
      <button
        type="button"
        aria-label="Next record"
        title="Next record"
        disabled={blocked || index == null || index >= last}
        onClick={() => index != null && go(index + 1)}
      >
        <ChevronRight aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Last record"
        title="Last record"
        disabled={blocked || !total || index === last}
        onClick={() => go(last)}
      >
        <ChevronLast aria-hidden="true" />
      </button>
      {onNew && (
        <button
          type="button"
          aria-label="New record"
          title="New record"
          disabled={disabled}
          onClick={onNew}
        >
          <Plus aria-hidden="true" />
        </button>
      )}
      {error && (
        <span className="rt-error" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
