/**
 * Form events (PRD §17.4, docs/decisions/form-events.md). Each event runs the
 * declarative action bound to it in `DesignForm.events`; nothing is a script.
 *
 * - onLoad: once when a record view of the form opens (detail, edit or create).
 * - onCurrent: each time the view moves to a record (a new record counts).
 * - beforeUpdate: before a create or edit is written; a failing action vetoes the save.
 * - afterUpdate: after the write committed; a failure is reported, the save stays.
 */
import { type ActionContext, type ActionResult, runAction } from "../automation/runner";
import type { DesignForm, FormEventName } from "../design/schema";

/** The action bound to `event`, if any. */
export const eventAction = (form: DesignForm, event: FormEventName): string | null =>
  form.events?.[event] || null;

/** Runs the action bound to `event`; null when none is bound. Never throws. */
export async function runFormEvent(
  form: DesignForm,
  event: FormEventName,
  ctx: ActionContext,
): Promise<ActionResult | null> {
  const actionId = eventAction(form, event);
  if (!actionId) return null;
  return runAction(actionId, ctx).catch((reason) => ({
    ok: false,
    error: reason instanceof Error ? reason.message : String(reason),
    steps: [],
    results: {},
  }));
}

/** Why a before update action stopped the save, or null when it allows it. */
export function vetoMessage(result: ActionResult | null): string | null {
  if (!result || result.ok) return null;
  if (result.cancelled) return "Save cancelled.";
  return result.error || "The record was not saved.";
}

/** The problem to show when an after update action failed, or null. */
export function afterUpdateProblem(result: ActionResult | null): string | null {
  if (!result || result.ok || result.cancelled) return null;
  return `Saved, but the after update action failed: ${result.error ?? "unknown error"}`;
}

/** Events to raise when a view shows `recordKey`: on load once per view, on current per record. */
export function eventsToRaise(
  state: { opened: boolean; current: string | null },
  recordKey: string,
): FormEventName[] {
  const events: FormEventName[] = [];
  if (!state.opened) events.push("onLoad");
  if (state.current !== recordKey) events.push("onCurrent");
  state.opened = true;
  state.current = recordKey;
  return events;
}
