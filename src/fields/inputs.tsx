import {
  Bold,
  Download,
  Italic,
  List,
  ListOrdered,
  Paperclip,
  Trash2,
  Underline,
} from "lucide-react";
import { type ChangeEvent, useEffect, useRef, useState } from "react";
import { asTauriError } from "../lib/api";
import { chooseAttachmentDestination } from "../lib/dialog";
import { MAX_ATTACHMENT_BYTES, saveRecordAttachment, uploadRecordAttachment } from "./api";
import { applyMask, maskTemplate, parseMask } from "./mask";
import { sanitizeRichText } from "./richtext";
import { attachmentsValue, choicesValue, fileSize, parseAttachments, parseChoices } from "./values";

/** Props a field input shares with the plain inputs in `runtime/controls.tsx`. */
export type CommonInput = {
  id: string;
  label: string;
  value: unknown;
  onChange: (value: unknown) => void;
  onBlur: () => void;
  disabled: boolean;
  "aria-invalid"?: boolean;
  "aria-describedby"?: string;
  "aria-required"?: boolean;
};

const aria = (props: CommonInput) => ({
  "aria-invalid": props["aria-invalid"],
  "aria-describedby": props["aria-describedby"],
  "aria-required": props["aria-required"],
});

/** Text entry that fits what is typed into an input mask. */
export function MaskedInput(props: CommonInput & { mask: string }) {
  const { id, value, onChange, onBlur, disabled, mask } = props;
  const parsed = parseMask(mask);
  const text = value == null ? "" : String(value);
  return (
    <input
      id={id}
      {...aria(props)}
      type="text"
      disabled={disabled}
      placeholder={maskTemplate(parsed)}
      value={applyMask(parsed, text).display}
      onBlur={onBlur}
      onChange={(e) => {
        const next = applyMask(parsed, e.target.value).stored;
        onChange(next === "" ? null : next);
      }}
    />
  );
}

const FORMATS = [
  { command: "bold", label: "Bold", icon: Bold },
  { command: "italic", label: "Italic", icon: Italic },
  { command: "underline", label: "Underline", icon: Underline },
  { command: "insertUnorderedList", label: "Bulleted list", icon: List },
  { command: "insertOrderedList", label: "Numbered list", icon: ListOrdered },
];

/** Formatted text: a content-editable box whose HTML is sanitized on every change. */
export function RichTextInput(props: CommonInput) {
  const { id, label, value, onChange, onBlur, disabled } = props;
  const html = sanitizeRichText(value == null ? "" : String(value));
  const box = useRef<HTMLDivElement>(null);
  const emitted = useRef<string | null>(null);
  // Only a value from outside (another record, an undo) replaces the box's content.
  useEffect(() => {
    if (box.current && html !== emitted.current) {
      box.current.innerHTML = html;
      emitted.current = html;
    }
  }, [html]);
  if (disabled)
    return (
      <div
        id={id}
        className="rt-richtext rt-richtext-view"
        aria-label={label}
        // biome-ignore lint/security/noDangerouslySetInnerHtml: sanitized by sanitizeRichText
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
  const emit = () => {
    const next = sanitizeRichText(box.current?.innerHTML ?? "");
    emitted.current = next;
    onChange(next === "" ? null : next);
  };
  return (
    <div className="rt-richtext">
      <div className="rt-richtext-tools" role="toolbar" aria-label={`${label} formatting`}>
        {FORMATS.map(({ command, label: name, icon: Icon }) => (
          <button
            key={command}
            type="button"
            aria-label={name}
            title={name}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              box.current?.focus();
              document.execCommand?.(command);
              emit();
            }}
          >
            <Icon aria-hidden="true" />
          </button>
        ))}
      </div>
      <div
        ref={box}
        id={id}
        {...aria(props)}
        className="rt-richtext-box"
        role="textbox"
        aria-multiline="true"
        aria-label={label}
        contentEditable
        suppressContentEditableWarning
        tabIndex={0}
        onInput={emit}
        onBlur={onBlur}
      />
    </div>
  );
}

/** Several choices from a fixed list, stored as a JSON array. */
export function MultiSelectInput(props: CommonInput & { options: string[] }) {
  const { id, label, value, onChange, onBlur, disabled, options } = props;
  const chosen = parseChoices(value);
  const all = [...options, ...chosen.filter((c) => !options.includes(c))];
  const toggle = (option: string, on: boolean) => {
    const next = all.filter((o) => (o === option ? on : chosen.includes(o)));
    onChange(choicesValue(next));
  };
  return (
    <fieldset id={id} className="rt-multiselect" {...aria(props)} onBlur={onBlur}>
      <legend className="sr-only">{label}</legend>
      {all.length === 0 && <span className="rt-muted">No choices</span>}
      {all.map((option) => (
        <label key={option}>
          <input
            type="checkbox"
            disabled={disabled}
            checked={chosen.includes(option)}
            onChange={(e) => toggle(option, e.target.checked)}
          />
          {option}
        </label>
      ))}
    </fieldset>
  );
}

/** A picked file's bytes; FileReader works in every webview and in jsdom. */
const fileBytes = (file: Blob) =>
  new Promise<Uint8Array>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error("The file could not be read"));
    reader.readAsArrayBuffer(file);
  });

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}

/** The files of an attachment field: add, save a copy, or remove. */
export function AttachmentInput(
  props: CommonInput & { table: string | null; column: string | null },
) {
  const { id, label, value, onChange, onBlur, disabled, table, column } = props;
  const files = parseAttachments(value);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const add = async (event: ChangeEvent<HTMLInputElement>) => {
    const picked = [...(event.target.files ?? [])];
    event.target.value = "";
    if (!picked.length || !table || !column) return;
    setBusy(true);
    setError("");
    const added = [];
    try {
      for (const file of picked) {
        if (file.size > MAX_ATTACHMENT_BYTES)
          throw new Error(
            `${file.name} is ${fileSize(file.size)}; attachments can be at most ${fileSize(MAX_ATTACHMENT_BYTES)}.`,
          );
        const bytes = await fileBytes(file);
        added.push(
          await uploadRecordAttachment({
            table,
            column,
            name: file.name,
            mime: file.type || null,
            contentBase64: toBase64(bytes),
          }),
        );
      }
    } catch (reason) {
      setError(asTauriError(reason).message);
    } finally {
      if (added.length) onChange(attachmentsValue([...files, ...added]));
      setBusy(false);
      onBlur();
    }
  };
  const saveCopy = async (fileId: string, name: string) => {
    try {
      const path = await chooseAttachmentDestination(name);
      if (path) await saveRecordAttachment(fileId, path);
    } catch (reason) {
      setError(asTauriError(reason).message);
    }
  };
  return (
    <div className="rt-attachments" {...aria(props)}>
      {files.length === 0 && <p className="rt-muted">No files</p>}
      <ul aria-label={`${label} files`}>
        {files.map((file) => (
          <li key={file.id}>
            <Paperclip aria-hidden="true" />
            <span>{file.name}</span>
            <small>{fileSize(file.size)}</small>
            <button
              type="button"
              aria-label={`Save ${file.name}`}
              onClick={() => saveCopy(file.id, file.name)}
            >
              <Download aria-hidden="true" />
            </button>
            {!disabled && (
              <button
                type="button"
                aria-label={`Remove ${file.name}`}
                onClick={() => {
                  onChange(attachmentsValue(files.filter((f) => f.id !== file.id)));
                  onBlur();
                }}
              >
                <Trash2 aria-hidden="true" />
              </button>
            )}
          </li>
        ))}
      </ul>
      {!disabled && (
        <label className="rt-attach-add">
          {busy ? "Adding…" : "Add files"}
          <input id={id} type="file" multiple disabled={busy} onChange={add} />
        </label>
      )}
      {error && (
        <p className="rt-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
