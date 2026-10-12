/** Laid-out report pages. All coordinates are points from the top-left page corner. */

export interface TextLine {
  text: string;
  /** Left edge of the line (alignment already applied). */
  x: number;
  /** Baseline. */
  y: number;
  width: number;
}

/** A path operation in page points: move, line, cubic Bézier curve, or close. */
export type PathOp =
  | ["M", number, number]
  | ["L", number, number]
  | ["C", number, number, number, number, number, number]
  | ["Z"];

/** One chart mark. Colors are `#rrggbb`; null means no fill or no stroke. */
export interface PathItem {
  kind: "path";
  ops: PathOp[];
  fill: string | null;
  stroke: string | null;
  lineWidth: number;
}

export type TextItem = Extract<PositionedItem, { kind: "text" }>;

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
    }
  | {
      /**
       * A chart: paths and text marks in page coordinates, drawn in order.
       * It never splits across pages; `title` labels it for assistive technology.
       */
      kind: "chart";
      componentId: string;
      x: number;
      y: number;
      w: number;
      h: number;
      title: string;
      marks: (PathItem | TextItem)[];
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
}

/** Rounds to 1/100 pt so golden layouts don't carry floating-point noise. */
export const r2 = (n: number) => Math.round(n * 100) / 100 + 0;

const movePath = (ops: PathOp[], dy: number): PathOp[] =>
  ops.map((op) => {
    switch (op[0]) {
      case "M":
      case "L":
        return [op[0], op[1], r2(op[2] + dy)];
      case "C":
        return ["C", op[1], r2(op[2] + dy), op[3], r2(op[4] + dy), op[5], r2(op[6] + dy)];
      case "Z":
        return op;
    }
  });

/** A chart item moved down by `dy` with all of its marks. */
export function moveChart(
  item: Extract<PositionedItem, { kind: "chart" }>,
  dy: number,
): PositionedItem {
  return {
    ...item,
    y: r2(item.y + dy),
    marks: item.marks.map((mark) =>
      mark.kind === "path"
        ? { ...mark, ops: movePath(mark.ops, dy) }
        : {
            ...mark,
            y: r2(mark.y + dy),
            lines: mark.lines.map((line) => ({ ...line, y: r2(line.y + dy) })),
          },
    ),
  };
}
