/**
 * Access-style input masks. A mask has up to three `;`-separated sections:
 * the pattern, `0` to store literal characters with the value (blank or `1` stores
 * only what was typed), and the placeholder character shown for empty slots (`_`).
 *
 * Pattern characters: `0` digit, `9` optional digit, `#` optional digit, space, + or -,
 * `L` letter, `?` optional letter, `A` letter or digit, `a` optional letter or digit,
 * `&` any character, `C` optional any character, `>` upper-cases and `<` lower-cases
 * what follows, `!` is ignored, `\x` and `"text"` are literals, and anything else is
 * a literal too.
 */

type Slot =
  | { kind: "literal"; char: string }
  | { kind: "input"; accepts: RegExp; required: boolean; casing: "upper" | "lower" | null };

export interface ParsedMask {
  slots: Slot[];
  storeLiterals: boolean;
  placeholder: string;
}

const SLOTS: Record<string, { accepts: RegExp; required: boolean }> = {
  "0": { accepts: /\d/, required: true },
  "9": { accepts: /\d/, required: false },
  "#": { accepts: /[\d +-]/, required: false },
  L: { accepts: /\p{L}/u, required: true },
  "?": { accepts: /\p{L}/u, required: false },
  A: { accepts: /[\p{L}\d]/u, required: true },
  a: { accepts: /[\p{L}\d]/u, required: false },
  "&": { accepts: /[^]/, required: true },
  C: { accepts: /[^]/, required: false },
};

/** Splits on `;` outside quotes and escapes. */
function sections(mask: string): string[] {
  const out = [""];
  let quoted = false;
  for (let i = 0; i < mask.length; i++) {
    const char = mask[i];
    if (char === "\\" && i + 1 < mask.length) {
      out[out.length - 1] += char + mask[++i];
      continue;
    }
    if (char === '"') quoted = !quoted;
    if (char === ";" && !quoted) out.push("");
    else out[out.length - 1] += char;
  }
  return out;
}

export function parseMask(mask: string): ParsedMask {
  const [pattern = "", store = "", placeholder = ""] = sections(mask);
  const slots: Slot[] = [];
  let casing: "upper" | "lower" | null = null;
  const chars = [...pattern];
  for (let i = 0; i < chars.length; i++) {
    const char = chars[i];
    if (char === "\\" && i + 1 < chars.length) slots.push({ kind: "literal", char: chars[++i] });
    else if (char === '"') {
      while (++i < chars.length && chars[i] !== '"')
        slots.push({ kind: "literal", char: chars[i] });
    } else if (char === ">") casing = "upper";
    else if (char === "<") casing = "lower";
    else if (char === "!") continue;
    else if (char in SLOTS) slots.push({ kind: "input", ...SLOTS[char], casing });
    else slots.push({ kind: "literal", char });
  }
  return {
    slots,
    storeLiterals: store.trim() === "0",
    placeholder: [...placeholder][0] ?? "_",
  };
}

export interface MaskResult {
  /** What the input shows: typed characters with the literals between them. */
  display: string;
  /** What is stored: `display`, or only the typed characters when literals are not stored. */
  stored: string;
  /** Every required slot is filled. */
  complete: boolean;
  /** Characters of the input that fit no slot. */
  rejected: number;
}

const cased = (char: string, casing: "upper" | "lower" | null) =>
  casing === "upper" ? char.toUpperCase() : casing === "lower" ? char.toLowerCase() : char;

/**
 * Fits typed text into the mask. Literals in the text are matched where the mask has
 * them and skipped elsewhere, so both stored forms (with or without literals) and
 * pasted text format the same way. Literals after the last typed character are left
 * off, so deleting backwards never gets stuck on one.
 */
export function applyMask(mask: ParsedMask, text: string): MaskResult {
  const input = [...text];
  const literals = new Set(mask.slots.flatMap((s) => (s.kind === "literal" ? [s.char] : [])));
  let display = "";
  let data = "";
  let pending = "";
  let rejected = 0;
  let complete = true;
  let at = 0;
  for (const slot of mask.slots) {
    if (slot.kind === "literal") {
      if (input[at] === slot.char) at++;
      pending += slot.char;
      continue;
    }
    // Skip characters this slot cannot take: stray literals silently, anything else counts.
    while (at < input.length && !slot.accepts.test(input[at])) {
      if (!slot.required) break;
      if (!literals.has(input[at])) rejected++;
      at++;
    }
    if (at < input.length && slot.accepts.test(input[at])) {
      const char = cased(input[at++], slot.casing);
      display += pending + char;
      data += char;
      pending = "";
    } else if (slot.required) {
      complete = false;
    }
  }
  for (; at < input.length; at++) if (!literals.has(input[at])) rejected++;
  return { display, stored: mask.storeLiterals ? display : data, complete, rejected };
}

/** The empty mask as shown in a placeholder, e.g. `(___) ___-____`. */
export const maskTemplate = (mask: ParsedMask) =>
  mask.slots.map((slot) => (slot.kind === "literal" ? slot.char : mask.placeholder)).join("");

/** A message when `value` does not fill the mask, else null. Blank values pass. */
export function maskProblem(maskText: string, value: unknown, label: string): string | null {
  if (value === null || value === undefined || String(value) === "") return null;
  const mask = parseMask(maskText);
  const result = applyMask(mask, String(value));
  if (result.rejected > 0 || !result.complete) return `${label} must match ${maskTemplate(mask)}.`;
  return null;
}

/** Stored text for typed text, or throws a readable message when it does not fit. */
export function maskedText(maskText: string, text: string, label: string): string {
  const problem = maskProblem(maskText, text, label);
  if (problem) throw new Error(problem);
  return text === "" ? "" : applyMask(parseMask(maskText), text).stored;
}

/** A mask that has no slot to type into cannot be used. */
export const maskIssue = (maskText: string) =>
  parseMask(maskText).slots.some((slot) => slot.kind === "input")
    ? null
    : "The input mask has no characters to type into.";
