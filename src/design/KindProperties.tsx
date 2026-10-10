import { Plus, Trash2 } from "lucide-react";
import { newId } from "../lib/utils";
import { filterNames } from "../runtime/conditions";
import { ExpressionField } from "./ExpressionField";
import { embeddableForms } from "./nesting";
import { removeTab } from "./operations";
import type { DesignControl, DesignForm } from "./schema";
import { useColumns } from "./useColumns";
import { useDesignEditor } from "./useDesignEditor";

type Props = { form: DesignForm; control: DesignControl };

export function ColumnSelect({
  label,
  value,
  columns,
  onChange,
}: {
  label: string;
  value?: string | null;
  columns: string[];
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <select value={value ?? ""} onChange={(e) => onChange(e.target.value)}>
        <option value="">—</option>
        {columns.map((c) => (
          <option key={c}>{c}</option>
        ))}
        {value && !columns.includes(value) && <option>{value}</option>}
      </select>
    </label>
  );
}

/** Tab pages of a tab group. */
export function TabsProperties({ form, control }: Props) {
  const { editForm, editControl } = useDesignEditor();
  const change = (patch: Partial<DesignControl>) => editControl(form.id, control.id, patch);
  return (
    <fieldset className="fd-fieldset">
      <legend>Tabs</legend>
      {(control.tabs ?? []).map((tab, index) => (
        <div className="fd-row" key={tab.id}>
          <label>
            Tab {index + 1} label
            <input
              value={tab.label}
              onChange={(e) =>
                change({
                  tabs: (control.tabs ?? []).map((t) =>
                    t.id === tab.id ? { ...t, label: e.target.value } : t,
                  ),
                })
              }
            />
          </label>
          <button
            type="button"
            aria-label={`Remove tab ${tab.label}`}
            onClick={() => editForm(form.id, (f) => removeTab(f, control.id, tab.id), "Remove tab")}
          >
            <Trash2 aria-hidden="true" />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          change({
            tabs: [
              ...(control.tabs ?? []),
              { id: newId(), label: `Tab ${(control.tabs ?? []).length + 1}` },
            ],
          })
        }
      >
        <Plus aria-hidden="true" />
        Add tab
      </button>
    </fieldset>
  );
}

/** Child table, link columns, and child form of a related-record list. */
export function RelatedListProperties({
  form,
  control,
  columns,
  tables,
}: Props & { columns: string[]; tables: string[] }) {
  const { design, editControl } = useDesignEditor();
  const change = (patch: Partial<DesignControl>) => editControl(form.id, control.id, patch);
  const childColumns = useColumns(control.related?.table);
  // Nested forms stay within three levels and never embed a form that embeds this one.
  const options = new Set(embeddableForms(design.forms, form).map((f) => f.id));
  return (
    <fieldset className="fd-fieldset">
      <legend>Related records</legend>
      <label>
        Child table
        <select
          value={control.related?.table ?? ""}
          onChange={(e) =>
            change({
              related: {
                table: e.target.value,
                foreignKey: "",
                parentColumn: columns[0] ?? "",
                columns: [],
                formId: null,
              },
            })
          }
        >
          <option value="">—</option>
          {tables.map((t) => (
            <option key={t}>{t}</option>
          ))}
        </select>
      </label>
      <ColumnSelect
        label="Foreign key column"
        value={control.related?.foreignKey}
        columns={childColumns}
        onChange={(foreignKey) =>
          control.related && change({ related: { ...control.related, foreignKey } })
        }
      />
      <ColumnSelect
        label="Parent column"
        value={control.related?.parentColumn}
        columns={columns}
        onChange={(parentColumn) =>
          control.related && change({ related: { ...control.related, parentColumn } })
        }
      />
      <ExpressionField
        label="Row filter"
        value={control.related?.filter}
        names={filterNames(childColumns)}
        placeholder="record.status <> 'void'"
        onChange={(filter) =>
          control.related && change({ related: { ...control.related, filter } })
        }
      />
      <label>
        Child form
        <select
          value={control.related?.formId ?? ""}
          onChange={(e) =>
            control.related &&
            change({ related: { ...control.related, formId: e.target.value || null } })
          }
        >
          <option value="">Generated from the table</option>
          {design.forms
            .filter((f) => f.id === control.related?.formId || options.has(f.id))
            .map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
        </select>
      </label>
    </fieldset>
  );
}
