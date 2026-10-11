import { maskIssue, maskTemplate, parseMask } from "./mask";

/** Input mask editor with the empty mask previewed, e.g. `(___) ___-____`. */
export function MaskField({
  value,
  onChange,
  label = "Input mask",
  name,
}: {
  value?: string | null;
  onChange: (value: string | null) => void;
  label?: string;
  /** Accessible name when the visible label alone is ambiguous, e.g. one per column. */
  name?: string;
}) {
  const mask = value ?? "";
  const issue = mask ? maskIssue(mask) : null;
  return (
    <label>
      {label}
      <input
        aria-label={name}
        value={mask}
        placeholder="e.g. (000) 000-0000"
        onChange={(e) => onChange(e.target.value || null)}
      />
      {mask && (
        <span className={issue ? "fd-error" : "fd-hint"} role={issue ? "alert" : undefined}>
          {issue ?? `Entry looks like ${maskTemplate(parseMask(mask))}`}
        </span>
      )}
    </label>
  );
}
