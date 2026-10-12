/**
 * Minimal deterministic PDF 1.4 writer for laid-out reports (no dependencies).
 *
 * - Fonts: standard Helvetica / Helvetica-Bold, WinAnsiEncoding (not embedded),
 *   plus embedded subsets of the bundled fallback fonts for other characters
 *   (see pdf-fonts.ts).
 * - Graphics: text, lines, rectangles in gray levels; chart paths in RGB; JPEG and opaque PNG
 *   images passed through, other PNGs decoded by Rust with an alpha soft mask
 *   (see pdf-images.ts).
 * - Determinism: fixed object order, uncompressed content streams, no random
 *   file id, and the CreationDate comes from the options. Same input, same bytes.
 */
import type { PathItem, PositionedItem, ReportDocument } from "./engine";
import { textRuns, winAnsiCode } from "./engine";
import { cidMap, cidString, fallbackFontName, fontObjects, type PdfFontSubset } from "./pdf-fonts";
import { type DecodedPng, decodedImage, type PdfImage, pdfImage } from "./pdf-images";

export interface PdfAsset {
  mediaType: string;
  data: Uint8Array;
}

export interface PdfOptions {
  title?: string;
  /** ISO timestamp written as /CreationDate. Required so output is reproducible. */
  creationDate: string;
  assets?: Record<string, PdfAsset>;
  /** Fallback font subsets from `prepare_report_pdf`; without them those characters print as `?`. */
  fonts?: PdfFontSubset[];
  /** PNG assets decoded by `prepare_report_pdf`, by asset id. */
  decodedImages?: Record<string, DecodedPng>;
}

/** Formats a number for content streams: at most 2 decimals, no exponent, no -0. */
export function num(n: number): string {
  const r = Math.round(n * 100) / 100;
  if (!Number.isFinite(r) || r === 0) return "0";
  const s = r.toFixed(2);
  return s.replace(/\.?0+$/, "");
}

/** A PDF literal string in WinAnsi with `\`, `(`, `)` and non-ASCII bytes escaped. */
export function pdfString(text: string): string {
  let out = "(";
  for (const char of text) {
    const code = winAnsiCode(char);
    if (code === 0x5c || code === 0x28 || code === 0x29) out += `\\${String.fromCharCode(code)}`;
    else if (code < 32 || code > 126) out += `\\${code.toString(8).padStart(3, "0")}`;
    else out += String.fromCharCode(code);
  }
  return `${out})`;
}

/** `D:YYYYMMDDHHmmSSZ` from an ISO timestamp (UTC). */
export function pdfDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "D:19700101000000Z";
  const p = (n: number) => String(n).padStart(2, "0");
  return `D:${d.getUTCFullYear()}${p(d.getUTCMonth() + 1)}${p(d.getUTCDate())}${p(d.getUTCHours())}${p(d.getUTCMinutes())}${p(d.getUTCSeconds())}Z`;
}

const ascii = (s: string) => {
  const bytes = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) bytes[i] = s.charCodeAt(i) & 0xff;
  return bytes;
};

type Fonts = Map<number, Map<number, number>>;

/** Text operators of one line: Helvetica runs as WinAnsi strings, fallback runs as CIDs. */
function lineOps(
  item: Extract<PositionedItem, { kind: "text" }>,
  line: { text: string; x: number; y: number },
  y: string,
  fonts: Fonts,
  current: { font: string },
): string {
  const base = item.bold ? "/F2" : "/F1";
  const size = num(item.fontSize);
  const ops: string[] = [];
  for (const run of textRuns(line.text, item.fontSize, item.bold)) {
    const cids = fonts.get(run.font);
    const font = cids ? fallbackFontName(run.font) : base;
    if (font !== current.font) ops.push(`${font} ${size} Tf`);
    current.font = font;
    const text = cids ? cidString(cids, run.text) : pdfString(run.text);
    const show = `1 0 0 1 ${num(line.x + run.dx)} ${y} Tm ${text} Tj`;
    // Fallback fonts have no bold face: stroke the outlines (render mode 2) instead.
    ops.push(
      cids && item.bold
        ? `${num(item.gray)} G ${num(item.fontSize * 0.03)} w 2 Tr ${show} 0 Tr`
        : show,
    );
  }
  return ops.join("\n");
}

/** `r g b` of a `#rrggbb` color, each 0–1. */
export function rgb(color: string): string {
  return [0, 1, 2]
    .map((i) => num(Number.parseInt(color.slice(1 + i * 2, 3 + i * 2), 16) / 255 || 0))
    .join(" ");
}

/** A chart path: filled, stroked, or both, with round joins like the SVG preview. */
function pathOps(item: PathItem, y: (v: number) => string): string {
  const stroke = !!item.stroke && item.lineWidth > 0;
  const fill = item.fill !== null;
  if ((!stroke && !fill) || !item.ops.length) return "";
  const segments = item.ops.map((op) => {
    switch (op[0]) {
      case "M":
        return `${num(op[1])} ${y(op[2])} m`;
      case "L":
        return `${num(op[1])} ${y(op[2])} l`;
      case "C":
        return `${num(op[1])} ${y(op[2])} ${num(op[3])} ${y(op[4])} ${num(op[5])} ${y(op[6])} c`;
      case "Z":
        return "h";
    }
  });
  return [
    "q",
    fill ? `${rgb(item.fill as string)} rg` : "",
    stroke ? `${rgb(item.stroke as string)} RG ${num(item.lineWidth)} w 1 j` : "",
    segments.join(" "),
    stroke && fill ? "B" : stroke ? "S" : "f",
    "Q",
  ]
    .filter(Boolean)
    .join("\n");
}

function itemOps(
  item: PositionedItem | PathItem,
  pageHeight: number,
  image: (id: string) => string | null,
  fonts: Fonts,
): string {
  const y = (v: number) => num(pageHeight - v);
  switch (item.kind) {
    case "rect": {
      const stroke = item.lineWidth > 0;
      const fill = item.fill !== null;
      if (!stroke && !fill) return "";
      const op = stroke && fill ? "B" : stroke ? "S" : "f";
      return [
        "q",
        fill ? `${num(item.fill as number)} g` : "",
        stroke ? `${num(item.gray)} G ${num(item.lineWidth)} w` : "",
        `${num(item.x)} ${y(item.y + item.h)} ${num(item.w)} ${num(item.h)} re ${op}`,
        "Q",
      ]
        .filter(Boolean)
        .join("\n");
    }
    case "line":
      return `q ${num(item.gray)} G ${num(item.lineWidth)} w ${num(item.x)} ${y(item.y)} m ${num(item.x + item.w)} ${y(item.y + item.h)} l S Q`;
    case "text": {
      if (!item.lines.length) return "";
      const font = item.bold ? "/F2" : "/F1";
      const current = { font };
      const lines = item.lines.map((line) => lineOps(item, line, y(line.y), fonts, current));
      return ["BT", `${font} ${num(item.fontSize)} Tf ${num(item.gray)} g`, ...lines, "ET"].join(
        "\n",
      );
    }
    case "path":
      return pathOps(item, y);
    case "chart":
      return item.marks
        .map((mark) => itemOps(mark, pageHeight, image, fonts))
        .filter(Boolean)
        .join("\n");
    case "image": {
      const name = image(item.assetId);
      if (name)
        return `q ${num(item.w)} 0 0 ${num(item.h)} ${num(item.x)} ${y(item.y + item.h)} cm ${name} Do Q`;
      // Unsupported or missing image data: a crossed placeholder box.
      const x0 = num(item.x);
      const x1 = num(item.x + item.w);
      const y0 = y(item.y + item.h);
      const y1 = y(item.y);
      return `q 0.5 G 0.5 w ${x0} ${y0} ${num(item.w)} ${num(item.h)} re S ${x0} ${y0} m ${x1} ${y1} l ${x0} ${y1} m ${x1} ${y0} l S Q`;
    }
  }
}

/** Serializes a laid-out report to PDF 1.4 bytes. */
export function writePdf(doc: ReportDocument, options: PdfOptions): Uint8Array {
  // Objects: 1 catalog, 2 pages, 3-4 fonts, 5 info, images (+ soft mask), fallback fonts (5 each), pages.
  const images: { name: string; image: PdfImage; ref: number }[] = [];
  const imageNames = new Map<string, string | null>();
  let next = 6;
  for (const page of doc.pages)
    for (const item of page.items) {
      if (item.kind !== "image" || imageNames.has(item.assetId)) continue;
      const asset = options.assets?.[item.assetId];
      const decoded = options.decodedImages?.[item.assetId];
      const image = decoded
        ? decodedImage(decoded, fromBase64)
        : asset
          ? pdfImage(asset.mediaType, asset.data)
          : null;
      if (!image) {
        imageNames.set(item.assetId, null);
        continue;
      }
      const name = `/Im${images.length + 1}`;
      imageNames.set(item.assetId, name);
      images.push({ name, image, ref: next });
      next += image.smask ? 2 : 1;
    }
  const fonts = (options.fonts ?? []).map((font, i) => ({ font, ref: next + i * 5 }));
  next += fonts.length * 5;
  const fontCids: Fonts = new Map(fonts.map(({ font }) => [font.index, cidMap(font)]));
  const firstPage = next;
  const pageRef = (i: number) => firstPage + i * 2;

  const chunks: Uint8Array[] = [];
  let length = 0;
  const offsets: number[] = [];
  const write = (part: string | Uint8Array) => {
    const bytes = typeof part === "string" ? ascii(part) : part;
    chunks.push(bytes);
    length += bytes.length;
  };
  const object = (n: number, body: string) => {
    offsets[n] = length;
    write(`${n} 0 obj\n${body}\nendobj\n`);
  };
  const stream = (n: number, dict: string, data: Uint8Array) => {
    offsets[n] = length;
    write(`${n} 0 obj\n<< ${dict ? `${dict} ` : ""}/Length ${data.length} >>\nstream\n`);
    write(data);
    write("\nendstream\nendobj\n");
  };

  write("%PDF-1.4\n%âãÏÓ\n");
  object(1, "<< /Type /Catalog /Pages 2 0 R >>");
  const kids = doc.pages.map((_, i) => `${pageRef(i)} 0 R`).join(" ");
  object(2, `<< /Type /Pages /Kids [${kids}] /Count ${doc.pages.length} >>`);
  object(3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
  object(
    4,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
  );
  const title = options.title ? ` /Title ${pdfString(options.title)}` : "";
  object(
    5,
    `<<${title} /Producer (ixtable report engine) /CreationDate (${pdfDate(options.creationDate)}) >>`,
  );
  for (const { image, ref } of images) {
    const smask = image.smask ? ` /SMask ${ref + 1} 0 R` : "";
    stream(ref, `/Type /XObject /Subtype /Image ${image.dict}${smask}`, image.data);
    if (image.smask)
      stream(ref + 1, `/Type /XObject /Subtype /Image ${image.smask.dict}`, image.smask.data);
  }
  for (const { font, ref } of fonts)
    fontObjects(font, ref, fromBase64(font.dataBase64)).forEach((part, i) => {
      if (part.stream) {
        const data = part.stream.data;
        stream(ref + i, part.stream.dict, typeof data === "string" ? ascii(data) : data);
      } else object(ref + i, part.body ?? "");
    });
  const xobjects = images.length
    ? ` /XObject << ${images.map((img) => `${img.name} ${img.ref} 0 R`).join(" ")} >>`
    : "";
  const fontRefs = fonts.map(({ font, ref }) => ` ${fallbackFontName(font.index)} ${ref} 0 R`);
  doc.pages.forEach((page, i) => {
    const n = pageRef(i);
    object(
      n,
      `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${num(doc.width)} ${num(doc.height)}]` +
        ` /Resources << /Font << /F1 3 0 R /F2 4 0 R${fontRefs.join("")} >>${xobjects} >> /Contents ${n + 1} 0 R >>`,
    );
    const content = page.items
      .map((item) => itemOps(item, doc.height, (id) => imageNames.get(id) ?? null, fontCids))
      .filter(Boolean)
      .join("\n");
    stream(n + 1, "", ascii(content));
  });

  const size = firstPage + doc.pages.length * 2;
  const xref = length;
  write(`xref\n0 ${size}\n0000000000 65535 f \n`);
  for (let n = 1; n < size; n++) write(`${String(offsets[n]).padStart(10, "0")} 00000 n \n`);
  write(`trailer\n<< /Size ${size} /Root 1 0 R /Info 5 0 R >>\nstartxref\n${xref}\n%%EOF\n`);

  const out = new Uint8Array(length);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.length;
  }
  return out;
}

function fromBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/** Base64 for passing PDF bytes to Rust. */
export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}
