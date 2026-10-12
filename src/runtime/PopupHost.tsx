import { useEffect, useRef, useState } from "react";
import { DialogFrame } from "../components/DialogFrame";
import type { FormMode } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import { FormRenderer } from "./FormRenderer";
import { CLOSE_FORM_EVENT, OPEN_POPUP_EVENT, type PopupRequest } from "./popup";

type Open = PopupRequest & { key: number; settled: boolean };

/**
 * Shows popup forms as modal dialogs over the running app, newest on top. A popup closes
 * with the record it saved, with a `closeForm` step's value, or with null when dismissed
 * (Close, Cancel, or Escape). Without a popup open, `closeForm` returns to the previous
 * page (`onCloseWithoutPopup`).
 */
export function PopupHost({ onCloseWithoutPopup }: { onCloseWithoutPopup: () => void }) {
  const { config } = useDocumentConfig();
  const [popups, setPopups] = useState<Open[]>([]);
  const stack = useRef<Open[]>([]);
  const fallback = useRef(onCloseWithoutPopup);
  useEffect(() => {
    stack.current = popups;
    fallback.current = onCloseWithoutPopup;
  });

  const close = (popup: Open, value: unknown) => {
    // The first close wins: a save closes an embedded form, which also reports done.
    if (popup.settled) return;
    popup.settled = true;
    popup.resolve(value ?? null);
    setPopups((items) => items.filter((item) => item !== popup));
  };
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });

  useEffect(() => {
    let next = 0;
    const open = (event: Event) => {
      const detail = (event as CustomEvent<PopupRequest>).detail;
      if (!detail?.target) return;
      event.preventDefault();
      next += 1;
      const popup: Open = { ...detail, key: next, settled: false };
      setPopups((items) => [...items, popup]);
    };
    const closeTop = (event: Event) => {
      event.preventDefault();
      const value = (event as CustomEvent<{ value: unknown }>).detail?.value;
      const top = stack.current.at(-1);
      if (top) closeRef.current(top, value);
      else fallback.current();
    };
    window.addEventListener(OPEN_POPUP_EVENT, open);
    window.addEventListener(CLOSE_FORM_EVENT, closeTop);
    return () => {
      window.removeEventListener(OPEN_POPUP_EVENT, open);
      window.removeEventListener(CLOSE_FORM_EVENT, closeTop);
      // Leaving Run mode dismisses whatever is still open, so waiting actions finish.
      for (const popup of stack.current) if (!popup.settled) popup.resolve(null);
    };
  }, []);

  return (
    <>
      {popups.map((popup, index) => {
        const form = config.design?.forms.find((f) => f.id === popup.target.id);
        const name = form?.name ?? "Form";
        const top = index === popups.length - 1;
        return (
          <div key={popup.key} className="rt-popup-backdrop" inert={!top || undefined}>
            <DialogFrame
              className="rt-popup"
              role="dialog"
              aria-modal="true"
              aria-label={name}
              onClose={() => close(popup, null)}
            >
              <FormRenderer
                formId={popup.target.id}
                mode={popup.target.mode as FormMode | undefined}
                recordId={popup.target.recordId}
                params={popup.target.params}
                embedded
                onDone={() => close(popup, null)}
                onSaved={(record) => close(popup, record)}
              />
              <div className="rt-actions rt-popup-footer">
                <button
                  type="button"
                  aria-label={`Close ${name}`}
                  onClick={() => close(popup, null)}
                >
                  Close
                </button>
              </div>
            </DialogFrame>
          </div>
        );
      })}
    </>
  );
}
