import { Plus, Trash2 } from "lucide-react";
import { newId } from "../../lib/utils";
import { REPORT_SCOPE_NAMES } from "../model";
import type { ConditionStyle, ReportCondition } from "../types";
import { ExpressionInput } from "./fields";
import { FILLS, GRAYS } from "./styles";

const CONDITION_NAMES = [...REPORT_SCOPE_NAMES, "value"];

/** A select whose empty option leaves the setting unchanged. */
function StyleSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: number | null | undefined;
  options: [string, number | null][];
  onChange: (value: number | null | undefined) => void;
}) {
  return (
    <label>
      {label}
      <select
        value={value === undefined ? "keep" : value === null ? "" : String(value)}
        onChange={(e) =>
          onChange(
            e.target.value === "keep"
              ? undefined
              : e.target.value === ""
                ? null
                : Number(e.target.value),
          )
        }
      >
        <option value="keep">Unchanged</option>
        {options.map(([name, v]) => (
          <option key={name} value={v === null ? "" : String(v)}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Conditional formatting rules of a text component. The first rule whose
 * condition holds applies its style; `value` is the component's own value.
 */
export function ConditionsEditor({
  conditions,
  onChange,
}: {
  conditions: ReportCondition[];
  onChange: (conditions: ReportCondition[]) => void;
}) {
  const set = (id: string, patch: Partial<ReportCondition>) =>
    onChange(conditions.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  const setStyle = (rule: ReportCondition, patch: ConditionStyle) => {
    const style: ConditionStyle = { ...rule.style, ...patch };
    for (const key of Object.keys(style) as (keyof ConditionStyle)[])
      if (style[key] === undefined) delete style[key];
    set(rule.id, { style });
  };
  return (
    <fieldset>
      <legend>Conditional formatting</legend>
      {conditions.map((rule, i) => (
        <fieldset key={rule.id}>
          <legend>Rule {i + 1}</legend>
          <ExpressionInput
            label={`Rule ${i + 1} condition`}
            value={rule.when}
            placeholder="value < 0"
            names={CONDITION_NAMES}
            onChange={(when) => set(rule.id, { when })}
          />
          <label className="inline">
            <input
              type="checkbox"
              checked={!!rule.style.bold}
              onChange={(e) => setStyle(rule, { bold: e.target.checked || undefined })}
            />
            Rule {i + 1} bold
          </label>
          <div className="grid2">
            <StyleSelect
              label={`Rule ${i + 1} text`}
              value={rule.style.gray}
              options={GRAYS}
              onChange={(gray) => setStyle(rule, { gray: gray ?? undefined })}
            />
            <StyleSelect
              label={`Rule ${i + 1} fill`}
              value={rule.style.fill}
              options={FILLS}
              onChange={(fill) => setStyle(rule, { fill })}
            />
          </div>
          <button
            type="button"
            onClick={() => onChange(conditions.filter((r) => r.id !== rule.id))}
          >
            <Trash2 /> Remove rule {i + 1}
          </button>
        </fieldset>
      ))}
      <button
        type="button"
        onClick={() => onChange([...conditions, { id: newId(), when: "", style: { bold: true } }])}
      >
        <Plus /> Add rule
      </button>
    </fieldset>
  );
}
