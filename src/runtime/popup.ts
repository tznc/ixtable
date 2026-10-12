import type { NavigationTarget } from "../automation/runner";

/**
 * Popup forms (PRD §14, Phase 6). An `openForm` step with `popup` asks the running app
 * to show the form in a modal dialog and waits until it closes; the dialog resolves with
 * the value the form returns (its saved record, or a `closeForm` step's value), or null
 * when it was dismissed. A host (Run mode's PopupHost) answers the window events below
 * and calls `preventDefault()`; with no host the caller falls back to plain navigation.
 */
export const OPEN_POPUP_EVENT = "ixtable:open-popup";
export const CLOSE_FORM_EVENT = "ixtable:close-form";

export type PopupRequest = { target: NavigationTarget; resolve: (value: unknown) => void };

/** Asks the host to open `target` as a popup; `undefined` when no host handled it. */
export function requestPopup(target: NavigationTarget): Promise<unknown> {
  return new Promise((resolve) => {
    const detail: PopupRequest = { target, resolve };
    const event = new CustomEvent(OPEN_POPUP_EVENT, { detail, cancelable: true });
    if (window.dispatchEvent(event)) resolve(undefined);
  });
}

/** Closes the topmost popup form with `value`; false when no host handled it. */
export function requestClose(value: unknown): boolean {
  const event = new CustomEvent(CLOSE_FORM_EVENT, { detail: { value }, cancelable: true });
  return !window.dispatchEvent(event);
}
