import type { ControlKind, DesignForm } from "../design/schema";
import { formTable } from "../design/schema";
import type { DocumentConfig } from "../lib/types";
import { fieldSettingsFor } from "./values";

const FORMAT_KINDS = new Set(["richText", "attachment", "multiSelect"]);

/**
 * The form with field settings filled into its controls: a plain text control bound to a
 * rich-text, attachment, or multi-select field becomes that kind, a text control without
 * its own input mask uses its column's, and a multi-select without choices uses the
 * field's. Returns
 * the same object when nothing changes, so memoized consumers keep their identity.
 */
export function withFieldDefaults(
  form: DesignForm,
  config: Pick<DocumentConfig, "entities"> | null | undefined,
): DesignForm {
  const table = formTable(form);
  if (!table || !config?.entities?.some((e) => e.table === table && e.fields?.length)) return form;
  let changed = false;
  const controls = form.controls.map((control) => {
    const field = fieldSettingsFor(
      config,
      control.binding?.table || table,
      control.binding?.column,
    );
    if (!field) return control;
    if (
      FORMAT_KINDS.has(field.format ?? "") &&
      (control.kind === "text" || control.kind === "multiline")
    ) {
      changed = true;
      const kind = field.format as ControlKind;
      const options = field.options?.map((value) => ({ value, label: value }));
      return { ...control, kind, ...(kind === "multiSelect" ? { options } : {}) };
    }
    if (control.kind === "text" && !control.inputMask && field.inputMask) {
      changed = true;
      return { ...control, inputMask: field.inputMask };
    }
    if (control.kind === "multiSelect" && !control.options?.length && field.options?.length) {
      changed = true;
      return { ...control, options: field.options.map((value) => ({ value, label: value })) };
    }
    return control;
  });
  return changed ? { ...form, controls } : form;
}
