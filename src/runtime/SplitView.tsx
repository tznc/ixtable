import { useState } from "react";
import type { DesignForm, FormMode } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import type { RecordCursor } from "./cursor";
import { ListView } from "./ListView";
import { RecordNavBar } from "./RecordNavBar";
import { RecordView } from "./RecordView";
import { resolveForm } from "./registry";

type Pane = { mode: "detail" | "edit" | "create"; recordId?: unknown; cursor: RecordCursor | null };

type Props = {
  form: DesignForm;
  params?: Record<string, unknown>;
  onNavigate: (target: { kind: string; id: string; mode?: string; recordId?: unknown }) => void;
};

/**
 * Split mode (PRD §14, Phase 6): the list as a datasheet above the selected record's
 * detail view. Choosing a row shows it below instead of opening a new view; saving in
 * the detail pane re-reads the list. The detail form is `detailFormId`, else this form.
 */
export function SplitView({ form, params, onNavigate }: Props) {
  const { config } = useDocumentConfig();
  const detail = (form.detailFormId && resolveForm(config, form.detailFormId)) || form;
  const [pane, setPane] = useState<Pane | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [dirty, setDirty] = useState(false);
  const show = (recordId: unknown, cursor: RecordCursor) => {
    if (dirty) return;
    setPane({ mode: "detail", recordId, cursor });
  };
  const create = detail.modes.includes("create")
    ? () => !dirty && setPane({ mode: "create", cursor: null })
    : undefined;
  return (
    <div className="rt-split">
      <ListView
        form={form}
        params={params}
        onOpen={show}
        onCreate={() => create?.()}
        selected={pane?.mode === "create" ? null : (pane?.cursor?.index ?? null)}
        refresh={refresh}
      />
      <div className="rt-split-detail">
        {pane ? (
          <>
            {pane.mode !== "create" && form.navigationBar && (
              <RecordNavBar
                form={form}
                cursor={pane.cursor}
                onGo={show}
                onNew={create}
                disabled={dirty}
              />
            )}
            <RecordView
              form={detail}
              mode={pane.mode}
              recordId={pane.recordId}
              onMode={(next: FormMode, recordId) =>
                setPane((current) => ({
                  mode: next === "edit" || next === "create" ? next : "detail",
                  recordId,
                  cursor: current?.cursor ?? null,
                }))
              }
              onClose={() => {
                setPane(null);
                setRefresh((n) => n + 1);
              }}
              onNavigate={onNavigate}
              onDirty={setDirty}
              onSaved={() => setRefresh((n) => n + 1)}
            />
          </>
        ) : (
          <p className="rt-muted">Choose a record to see it here.</p>
        )}
      </div>
    </div>
  );
}
