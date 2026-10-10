import { useEffect, useId, useMemo, useState } from "react";
import type { DesignControl } from "../design/schema";
import { AttachmentInput, MaskedInput, MultiSelectInput, RichTextInput } from "../fields/inputs";
import { useDocumentConfig } from "../lib/config-store";
import { readAssetDataUrl } from "./api";
import {
  cachedRelationshipLabel,
  type Choice,
  queryChoices,
  relationshipChoices,
  relationshipLabel,
} from "./data";
import { filterInputsKey, type Tone, toneClass } from "./conditions";
import { formatted } from "./formState";
import { useDebounced } from "./useDebounced";
import { sameValue } from "./values";

export type FieldProps = {
  control: DesignControl;
  value: unknown;
  onChange: (value: unknown) => void;
  onBlur: () => void;
  /** Read-only display (detail mode, disabled by `enabledWhen`, or no permission). */
  readOnly: boolean;
  error?: string;
  /** Conditional style tone from the control's `styles`. */
  tone?: Tone | null;
  /** Scope for a relationship `filter`: the edited record as `parent`, plus `form`, `app`. */
  filterScope?: Record<string, unknown>;
  /** Multi-column relationship: current value of every key column (by form column). */
  keyValues?: Record<string, unknown>;
  /** Multi-column relationship: writes every key column of the chosen record. */
  onKeys?: (values: Record<string, unknown>) => void;
  /** The form's table, where an attachment control stores its files. */
  table?: string | null;
};

const dateInput = (value: unknown, kind: string) => {
  if (value == null || value === "") return "";
  const text = String(value);
  if (kind === "date") return text.slice(0, 10);
  if (kind === "time") return text.length > 8 ? text.slice(11, 19) : text;
  return text.replace(" ", "T").slice(0, 19);
};

const numberValue = (raw: string) => (raw === "" ? null : Number(raw));

/** Label + described input for one bound control. Keyboard accessible, labelled, with error text. */
export function Field({
  control,
  value,
  onChange,
  onBlur,
  readOnly,
  error,
  tone,
  filterScope,
  keyValues,
  onKeys,
  table,
}: FieldProps) {
  const id = useId();
  const errorId = `${id}-error`;
  const required = control.validation?.required ?? false;
  const common = {
    id,
    "aria-invalid": error ? true : undefined,
    "aria-describedby": error ? errorId : undefined,
    "aria-required": required || undefined,
    onBlur,
    disabled: readOnly,
  };
  let input;
  switch (control.kind) {
    case "multiline":
      input = (
        <textarea
          {...common}
          rows={3}
          value={value == null ? "" : String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
      break;
    case "number":
    case "decimal":
      input = (
        <input
          {...common}
          type="number"
          step={control.kind === "number" ? 1 : "any"}
          min={control.validation?.min ?? undefined}
          max={control.validation?.max ?? undefined}
          value={value == null ? "" : String(value)}
          onChange={(e) => onChange(numberValue(e.target.value))}
        />
      );
      break;
    case "boolean":
      input = (
        <input
          {...common}
          type="checkbox"
          role={control.variant === "toggle" ? "switch" : undefined}
          checked={value === true || value === 1 || value === "1" || value === "true"}
          onChange={(e) => onChange(e.target.checked)}
        />
      );
      break;
    case "date":
    case "time":
    case "datetime":
      input = (
        <input
          {...common}
          type={control.kind === "datetime" ? "datetime-local" : control.kind}
          step={control.kind === "date" ? undefined : 1}
          value={dateInput(value, control.kind)}
          onChange={(e) => onChange(e.target.value || null)}
        />
      );
      break;
    case "select":
      input = <SelectInput {...common} control={control} value={value} onChange={onChange} />;
      break;
    case "relationship":
      input = (
        <RelationshipInput
          {...common}
          control={control}
          value={value}
          onChange={onChange}
          readOnly={readOnly}
          filterScope={filterScope}
          keyValues={keyValues}
          onKeys={onKeys}
        />
      );
      break;
    case "richText":
    case "attachment":
    case "multiSelect": {
      const props = { ...common, label: control.label, value, onChange };
      input =
        control.kind === "richText" ? (
          <RichTextInput {...props} />
        ) : control.kind === "attachment" ? (
          <AttachmentInput
            {...props}
            table={control.binding?.table || table || null}
            column={control.binding?.column ?? null}
          />
        ) : (
          <MultiSelectInput
            {...props}
            options={(control.options ?? []).map((option) => option.value)}
          />
        );
      break;
    }
    default:
      input = control.inputMask ? (
        <MaskedInput
          {...common}
          label={control.label}
          value={value}
          onChange={onChange}
          mask={control.inputMask}
        />
      ) : (
        <input
          {...common}
          type="text"
          value={value == null ? "" : String(value)}
          onChange={(e) => onChange(e.target.value)}
        />
      );
  }
  return (
    <div className={`rt-field rt-${control.kind} ${toneClass(tone ?? null)}`.trim()}>
      <label htmlFor={id}>
        {control.label}
        {required && !readOnly && <span aria-hidden="true"> *</span>}
      </label>
      {input}
      {error && (
        <p id={errorId} className="rt-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

type InputProps = {
  id: string;
  control: DesignControl;
  value: unknown;
  onChange: (value: unknown) => void;
  onBlur: () => void;
  disabled: boolean;
};

function useChoices(control: DesignControl): Choice[] {
  const { config } = useDocumentConfig();
  const [loaded, setLoaded] = useState<Choice[]>([]);
  const queryId = control.optionsQueryId;
  useEffect(() => {
    if (!queryId) return;
    let live = true;
    queryChoices(config, queryId)
      .then((choices) => live && setLoaded(choices))
      .catch(() => live && setLoaded([]));
    return () => {
      live = false;
    };
  }, [config, queryId]);
  if (queryId) return loaded;
  return (control.options ?? []).map((option) => ({
    value: option.value,
    label: option.label || option.value,
  }));
}

function SelectInput({ control, value, onChange, ...rest }: InputProps) {
  const choices = useChoices(control);
  const index = choices.findIndex((choice) => sameValue(choice.value, value));
  return (
    <select
      {...rest}
      value={index < 0 ? (value == null || value === "" ? "" : "__current") : String(index)}
      onChange={(e) =>
        onChange(e.target.value === "" ? null : choices[Number(e.target.value)]?.value)
      }
    >
      <option value="">—</option>
      {index < 0 && value != null && value !== "" && (
        <option value="__current">{String(value)}</option>
      )}
      {choices.map((choice, i) => (
        <option key={i} value={String(i)}>
          {choice.label}
        </option>
      ))}
    </select>
  );
}

/**
 * Foreign-key lookup: searchable list of the target table's display column, read via DuckDB.
 * A multi-column key matches and writes every key column of the chosen record.
 */
function RelationshipInput({
  control,
  value,
  onChange,
  readOnly,
  filterScope,
  keyValues,
  onKeys,
  ...rest
}: InputProps & Pick<FieldProps, "filterScope" | "keyValues" | "onKeys"> & { readOnly: boolean }) {
  const relationship = control.relationship;
  // Reload choices only when a value the filter reads settles on a new value.
  const scopeKey = useDebounced(filterInputsKey(relationship?.filter, filterScope ?? {}), 250);
  const scope = useMemo(
    () => (scopeKey ? (JSON.parse(scopeKey)[1] as Record<string, unknown>) : {}),
    [scopeKey],
  );
  const [choiceError, setChoiceError] = useState("");
  const pairs = relationship?.keys && relationship.keys.length > 1 ? relationship.keys : null;
  // The stored key: the bound value, or target column -> value for a multi-column key.
  const key: unknown = pairs
    ? Object.fromEntries(pairs.map((pair) => [pair.target, keyValues?.[pair.column] ?? null]))
    : value;
  const keySignature = pairs ? JSON.stringify(key) : value;
  const hasValue = pairs
    ? pairs.every((pair) => keyValues?.[pair.column] != null && keyValues[pair.column] !== "")
    : value != null && value !== "";
  const [search, setSearch] = useState("");
  const [choices, setChoices] = useState<Choice[]>([]);
  // Starts from the last label shown for this key, so a reload never blanks the field.
  const [current, setCurrent] = useState(() =>
    relationship ? (cachedRelationshipLabel(relationship, key) ?? "") : "",
  );
  useEffect(() => {
    if (!relationship || readOnly) return;
    let live = true;
    const timer = setTimeout(
      () => {
        relationshipChoices(relationship, search, 50, scope)
          .then((items) => {
            if (!live) return;
            setChoices(items);
            setChoiceError("");
          })
          .catch((reason) => {
            if (!live) return;
            setChoices([]);
            setChoiceError(reason instanceof Error ? reason.message : String(reason));
          });
      },
      search ? 150 : 0,
    );
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [relationship, search, readOnly, scope]);
  const composite = !!pairs;
  useEffect(() => {
    if (!relationship) return;
    let live = true;
    const stored: unknown = composite ? JSON.parse(keySignature as string) : keySignature;
    const cached = cachedRelationshipLabel(relationship, stored);
    if (cached !== undefined) setCurrent(cached);
    relationshipLabel(relationship, stored)
      .then((label) => live && setCurrent(label))
      .catch(() => live && setCurrent(composite ? "" : String(stored ?? "")));
    return () => {
      live = false;
    };
  }, [relationship, composite, keySignature]);
  if (!relationship) return <input {...rest} type="text" value={String(value ?? "")} readOnly />;
  if (readOnly) return <input {...rest} type="text" value={current} readOnly />;
  const index = pairs
    ? choices.findIndex((choice) =>
        pairs.every((pair) => sameValue(choice.record?.[pair.target], keyValues?.[pair.column])),
      )
    : choices.findIndex((choice) => sameValue(choice.value, value));
  const choose = (choice: Choice | undefined) => {
    if (!pairs) return onChange(choice ? choice.value : null);
    onKeys?.(
      Object.fromEntries(pairs.map((pair) => [pair.column, choice?.record?.[pair.target] ?? null])),
    );
  };
  return (
    <div className="rt-lookup">
      <input
        type="search"
        aria-label={`Search ${control.label}`}
        placeholder="Search…"
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />
      <select
        {...rest}
        value={index >= 0 ? String(index) : hasValue ? "__current" : ""}
        onChange={(e) =>
          choose(e.target.value === "" ? undefined : choices[Number(e.target.value)])
        }
      >
        <option value="">—</option>
        {index < 0 && hasValue && <option value="__current">{current}</option>}
        {choices.map((choice, i) => (
          <option key={i} value={String(i)}>
            {choice.label}
          </option>
        ))}
      </select>
      {choiceError && (
        <p className="rt-error" role="alert">
          Choice filter: {choiceError}
        </p>
      )}
    </div>
  );
}

/** Read-only computed value with the control's format. */
export function ComputedValue({
  control,
  value,
  error,
  tone,
}: {
  control: DesignControl;
  value: unknown;
  error?: string;
  tone?: Tone | null;
}) {
  const id = useId();
  return (
    <div className={`rt-field rt-computed ${toneClass(tone ?? null)}`.trim()}>
      <span id={id} className="rt-label">
        {control.label}
      </span>
      <output aria-labelledby={id} title={error}>
        {error ? "—" : formatted(value, control.format)}
      </output>
    </div>
  );
}

/** Image control: the asset rendered as an image, with the control label as alt text. */
export function ImageView({ control }: { control: DesignControl }) {
  const assetId = control.assetId ?? "";
  const [state, setState] = useState<{ id: string; url?: string; error?: string }>({ id: "" });
  useEffect(() => {
    if (!assetId) return;
    let live = true;
    readAssetDataUrl(assetId)
      .then((url) => live && setState({ id: assetId, url }))
      .catch(
        (reason) =>
          live &&
          setState({
            id: assetId,
            error: reason instanceof Error ? reason.message : String(reason),
          }),
      );
    return () => {
      live = false;
    };
  }, [assetId]);
  const current = state.id === assetId ? state : { id: assetId };
  return (
    <figure className="rt-image">
      {current.url ? (
        <img src={current.url} alt={control.label} />
      ) : (
        <div className="rt-image-box" role="img" aria-label={control.label} title={current.error}>
          {!assetId ? "No image" : current.error ? "Image unavailable" : "Loading image…"}
        </div>
      )}
      <figcaption>{control.label}</figcaption>
    </figure>
  );
}
