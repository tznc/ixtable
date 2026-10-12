import type { DesignForm } from "./schema";

/** Related lists nest up to three levels below the form that opens them (PRD Phase 7). */
export const MAX_SUBFORM_DEPTH = 3;

/** Forms a form's related lists open (only lists that name a form). */
export const embeddedFormIds = (form: DesignForm): string[] =>
  form.controls.flatMap((c) =>
    c.kind === "relatedList" && c.related?.formId ? [c.related.formId] : [],
  );

/**
 * Levels of related lists below `formId`: 0 when it embeds no form, 1 when its
 * related lists open forms without their own, and so on. A cycle is infinitely deep.
 */
export function subformDepth(
  forms: DesignForm[],
  formId: string,
  path: Set<string> = new Set(),
): number {
  if (path.has(formId)) return Number.POSITIVE_INFINITY;
  const form = forms.find((f) => f.id === formId);
  if (!form) return 0;
  const ids = embeddedFormIds(form);
  if (!ids.length) return 0;
  const inner = new Set(path).add(formId);
  return 1 + Math.max(...ids.map((id) => subformDepth(forms, id, inner)));
}

/** Levels of related lists above `formId`: the longest chain of forms that embed it. */
export function subformHeight(
  forms: DesignForm[],
  formId: string,
  path: Set<string> = new Set(),
): number {
  if (path.has(formId)) return Number.POSITIVE_INFINITY;
  const inner = new Set(path).add(formId);
  const parents = forms.filter((f) => embeddedFormIds(f).includes(formId));
  return Math.max(0, ...parents.map((p) => 1 + subformHeight(forms, p.id, inner)));
}

/**
 * Forms a related list on `form` may open: never `form` itself or a form that
 * embeds it, and only when the whole chain stays within `MAX_SUBFORM_DEPTH`.
 */
export function embeddableForms(forms: DesignForm[], form: DesignForm): DesignForm[] {
  const above = subformHeight(forms, form.id);
  return forms.filter((child) => {
    if (child.id === form.id) return false;
    const below = subformDepth(forms, child.id, new Set([form.id]));
    return above + 1 + below <= MAX_SUBFORM_DEPTH;
  });
}
