import type { Attachment } from "./types";

/**
 * Splits a text asset into titled sections. The Access importer writes VBA as
 * one file with a `' ==== Module Helpers ====` line before each module and
 * each form's or report's code (src-tauri/src/access/convert/mod.rs).
 * Text without such lines is one untitled section.
 */
export interface TextSection {
  title: string;
  text: string;
}

const HEADER = /^' ==== (.+) ====\s*$/;

export function textSections(content: string): TextSection[] {
  const sections: TextSection[] = [];
  let current: TextSection | null = null;
  for (const line of content.replace(/\r\n?/g, "\n").split("\n")) {
    const header = HEADER.exec(line);
    if (header) {
      current = { title: header[1], text: "" };
      sections.push(current);
    } else if (current) {
      current.text += `${current.text ? "\n" : ""}${line}`;
    } else if (line.trim() || sections.length) {
      current = { title: "", text: line };
      sections.push(current);
    }
  }
  return sections.map((s) => ({ ...s, text: s.text.replace(/\s+$/, "") }));
}

/** Text assets the Assets tab can show (the Access VBA asset among them). */
export const isTextAsset = (asset: Pick<Attachment, "mediaType">) =>
  asset.mediaType.toLowerCase().startsWith("text/");
