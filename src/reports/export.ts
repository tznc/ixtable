import { prepareReportPdf } from "./api";
import { base64Bytes } from "./data";
import { fallbackGlyph, type ReportDocument } from "./engine";
import { type PdfOptions, writePdf } from "./pdf";
import { pdfImage } from "./pdf-images";

/** Characters of a laid-out report that print in a fallback font, as sorted code points. */
export function fallbackCodePoints(doc: ReportDocument): number[] {
  const cps = new Set<number>();
  const texts = doc.pages.flatMap((page) =>
    page.items.flatMap((item) =>
      item.kind === "chart" ? item.marks : item.kind === "text" ? [item] : [],
    ),
  );
  for (const item of texts)
    if (item.kind === "text")
      for (const line of item.lines)
        for (const char of line.text) if (fallbackGlyph(char)) cps.add(char.codePointAt(0) ?? 0);
  return [...cps].sort((a, b) => a - b);
}

/**
 * PDF bytes of a laid-out report. Asks Rust for subsets of the bundled fonts
 * and for PNGs the writer can't pass through (alpha, tRNS, interlacing,
 * 16-bit) only when the report needs them, so plain reports make no extra
 * call. `warnings` says which images print as placeholders and why.
 */
export async function reportPdfBytes(
  doc: ReportDocument,
  assets: Record<string, { mediaType: string; dataBase64: string }>,
  options: Omit<PdfOptions, "assets" | "fonts" | "decodedImages">,
): Promise<{ bytes: Uint8Array; warnings: string[] }> {
  const used = new Set(
    doc.pages.flatMap((p) => p.items.flatMap((i) => (i.kind === "image" ? [i.assetId] : []))),
  );
  const bytes = Object.fromEntries(
    Object.entries(assets).map(([id, a]) => [
      id,
      { mediaType: a.mediaType, data: base64Bytes(a.dataBase64) },
    ]),
  );
  const pngs = Object.keys(bytes).filter(
    (id) =>
      used.has(id) && bytes[id].mediaType === "image/png" && !pdfImage("image/png", bytes[id].data),
  );
  const codePoints = fallbackCodePoints(doc);
  const prepared =
    codePoints.length || pngs.length
      ? await prepareReportPdf(
          codePoints,
          pngs.map((id) => assets[id].dataBase64),
        )
      : { fonts: [], images: [], warnings: [] };
  const decodedImages = Object.fromEntries(
    pngs.flatMap((id, i) => (prepared.images[i] ? [[id, prepared.images[i]]] : [])),
  );
  const pdf = writePdf(doc, { ...options, assets: bytes, fonts: prepared.fonts, decodedImages });
  return { bytes: pdf, warnings: prepared.warnings };
}
