/** Laid-out report pages. All coordinates are points from the top-left page corner. */
import type { Report } from "../types";

export interface TextLine {
  text: string;
  /** Left edge of the line (alignment already applied). */
  x: number;
  /** Baseline. */
  y: number;
  width: number;
}

export type PositionedItem =
  | {
      kind: "text";
      componentId: string;
      x: number;
      y: number;
      w: number;
      h: number;
      text: string;
      lines: TextLine[];
      fontSize: number;
      bold: boolean;
      gray: number;
    }
  | {
      /** From (x, y) to (x + w, y + h); one of w and h is 0. */
      kind: "line";
      componentId: string;
      x: number;
      y: number;
      w: number;
      h: number;
      lineWidth: number;
      gray: number;
    }
  | {
      kind: "rect";
      componentId: string;
      x: number;
      y: number;
      w: number;
      h: number;
      /** Stroke width; 0 = no stroke. */
      lineWidth: number;
      gray: number;
      fill: number | null;
    }
  | {
      kind: "image";
      componentId: string;
      x: number;
      y: number;
      w: number;
      h: number;
      assetId: string;
      mediaType: string;
    };

export interface Page {
  number: number;
  items: PositionedItem[];
}

export interface ReportDiagnostic {
  componentId: string;
  message: string;
}

export interface ReportDocument {
  width: number;
  height: number;
  pages: Page[];
  /** Expression and data problems, one per component and message. */
  diagnostics: ReportDiagnostic[];
}

export type Row = Record<string, unknown>;

export interface LayoutOptions {
  /** Parameter values exposed as `params`. Defaults to the report's `params`. */
  params?: Record<string, unknown>;
  /** Clock for `today()` / `now()`. Pass a fixed value for reproducible output. */
  now?: Date;
  /** Rows of saved queries used by table components, by query id. */
  tables?: Record<string, Row[]>;
  /** Known application assets by id (images). Missing assets render as a labelled box. */
  assets?: Record<string, { mediaType: string }>;
  /** Subreport definitions and all their rows, by report id (each instance filters by its links). */
  subreports?: Record<string, SubreportData>;
}

export interface SubreportData {
  report: Report;
  rows: Row[];
}

/** Rounds to 1/100 pt so golden layouts don't carry floating-point noise. */
export const r2 = (n: number) => Math.round(n * 100) / 100 + 0;
