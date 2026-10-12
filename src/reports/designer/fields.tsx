import { useEffect, useId, useState } from "react";
import { check } from "../../expr";
import { REPORT_SCOPE_NAMES } from "../model";

/** Text input for a report expression with live `check()` diagnostics. */
export function ExpressionInput({
  label,
  value,
  onChange,
  placeholder,
  names = REPORT_SCOPE_NAMES,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  /** Names the expression may use. */
  names?: string[];
}) {
  const id = useId();
  const problems = value.trim() ? check(value, names) : [];
  return (
    <div className="report-expression">
      <label>
        {label}
        <input
          value={value}
          placeholder={placeholder}
          spellCheck={false}
          aria-invalid={problems.length > 0}
          aria-describedby={problems.length ? id : undefined}
          onChange={(e) => onChange(e.target.value)}
        />
      </label>
      {problems.length > 0 && (
        <span id={id} className="report-diagnostic" role="alert">
          {problems.map((p) => p.message).join("; ")}
        </span>
      )}
    </div>
  );
}

/**
 * Number input in points. Valid entries apply as typed; the text is kept as a draft so a
 * value below `min` or a cleared field can be retyped, and blur restores the stored value.
 */
export function PointInput({
  label,
  value,
  onChange,
  min = 0,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
}) {
  const shown = String(Number.isFinite(value) ? value : 0);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  return (
    <label>
      {label}
      <input
        type="number"
        min={min}
        step={1}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          const n = Number(e.target.value);
          if (e.target.value !== "" && Number.isFinite(n) && n >= min) onChange(n);
        }}
        onBlur={() => setDraft(shown)}
      />
    </label>
  );
}
