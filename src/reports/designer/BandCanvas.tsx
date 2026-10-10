import { type KeyboardEvent, type PointerEvent, useRef, useState } from "react";
import { type BandEntry, type BandKey, KIND_LABELS } from "../model";
import type { ReportComponent } from "../types";
import { bandMinHeight, clampGeometry, type Geometry } from "./geometry";

type Drag =
  | {
      kind: "move" | "resize";
      id: string;
      startX: number;
      startY: number;
      origin: Geometry;
      now: Geometry;
    }
  | { kind: "band"; key: BandKey; startY: number; origin: number; now: number };

function summary(c: ReportComponent): string {
  switch (c.kind) {
    case "staticText":
      return c.text;
    case "field":
    case "calculated":
      return c.expression ? `[${c.expression}]` : "[no expression]";
    case "table":
      return `Table (${c.columns.length} columns)`;
    case "image":
      return c.assetId ? "Image" : "Image (none)";
    case "subreport":
      return c.reportId ? "Subreport" : "Subreport (none)";
    default:
      return "";
  }
}

const capture = (e: PointerEvent<HTMLElement>) => {
  try {
    e.currentTarget.setPointerCapture?.(e.pointerId);
  } catch {
    /* pointer capture is best effort */
  }
};

interface Props {
  bands: BandEntry[];
  width: number;
  selectedBand: BandKey;
  selectedId: string | null;
  onSelectBand: (key: BandKey) => void;
  onSelect: (id: string, key: BandKey) => void;
  onGeometry: (id: string, geometry: Geometry) => void;
  onBandHeight: (key: BandKey, height: number) => void;
  onDelete: (id: string) => void;
}

/** Freeform report canvas in points (1pt = 1px): bands stacked vertically, components positioned absolutely. */
export function BandCanvas(props: Props) {
  const { bands, width, selectedBand, selectedId } = props;
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const setDragState = (next: Drag | null) => {
    dragRef.current = next;
    setDrag(next);
  };

  const onMove = (e: PointerEvent<HTMLElement>, bandHeight: number) => {
    const d = dragRef.current;
    if (!d) return;
    const dy = e.clientY - d.startY;
    if (d.kind === "band") {
      setDragState({ ...d, now: Math.max(0, Math.round(d.origin + dy)) });
      return;
    }
    const dx = e.clientX - d.startX;
    const next =
      d.kind === "move"
        ? { ...d.origin, x: d.origin.x + dx, y: d.origin.y + dy }
        : { ...d.origin, w: d.origin.w + dx, h: d.origin.h + dy };
    setDragState({ ...d, now: clampGeometry(next, width, bandHeight) });
  };
  const onUp = () => {
    const d = dragRef.current;
    setDragState(null);
    if (!d) return;
    if (d.kind === "band") {
      if (d.now !== d.origin) props.onBandHeight(d.key, d.now);
    } else if (JSON.stringify(d.now) !== JSON.stringify(d.origin)) props.onGeometry(d.id, d.now);
  };

  const onKey = (e: KeyboardEvent<HTMLElement>, c: ReportComponent, bandHeight: number) => {
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      props.onDelete(c.id);
      return;
    }
    const arrows: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    const dir = arrows[e.key];
    if (!dir) return;
    e.preventDefault();
    const step = e.shiftKey ? 10 : 1;
    const [dx, dy] = [dir[0] * step, dir[1] * step];
    const next = e.altKey ? { ...c, w: c.w + dx, h: c.h + dy } : { ...c, x: c.x + dx, y: c.y + dy };
    props.onGeometry(c.id, clampGeometry(next, width, bandHeight));
  };

  return (
    <div className="report-sheet" style={{ width }}>
      {bands.map((entry) => {
        const height =
          drag?.kind === "band" && drag.key === entry.key ? drag.now : entry.band.height;
        const minHeight = bandMinHeight(entry);
        return (
          <div key={entry.key} role="group" aria-label={`${entry.label} band`}>
            <button
              type="button"
              className="report-band-label"
              aria-pressed={selectedBand === entry.key}
              onClick={() => props.onSelectBand(entry.key)}
            >
              {entry.label} · {Math.round(height)} pt
            </button>
            <div
              className="report-band"
              style={{ height: Math.max(height, minHeight) }}
              onPointerDown={(e) => {
                if (e.target === e.currentTarget) props.onSelectBand(entry.key);
              }}
            >
              {entry.band.components.map((c) => {
                const live =
                  drag && drag.kind !== "band" && drag.id === c.id ? { ...c, ...drag.now } : c;
                const label = `${KIND_LABELS[c.kind]} ${summary(c)}`.trim();
                return (
                  <div
                    key={c.id}
                    role="button"
                    tabIndex={0}
                    aria-label={label}
                    aria-pressed={selectedId === c.id}
                    className={`report-component ${c.kind === "line" ? "line" : ""} ${selectedId === c.id ? "selected" : ""}`}
                    style={{
                      left: live.x,
                      top: live.y,
                      width: live.w,
                      height: live.h,
                      fontSize: c.kind === "table" ? 9 : (c.style?.fontSize ?? 10),
                      fontWeight: c.style?.bold ? 700 : 400,
                      textAlign: c.style?.align ?? "left",
                      borderWidth: c.kind === "rectangle" ? (c.style?.borderWidth ?? 1) : undefined,
                      borderStyle: c.kind === "rectangle" ? "solid" : undefined,
                    }}
                    onFocus={() => props.onSelect(c.id, entry.key)}
                    onKeyDown={(e) => onKey(e, c, height)}
                    onPointerDown={(e) => {
                      if (e.button !== 0) return;
                      props.onSelect(c.id, entry.key);
                      const resize = (e.target as HTMLElement).dataset.handle === "resize";
                      capture(e);
                      setDragState({
                        kind: resize ? "resize" : "move",
                        id: c.id,
                        startX: e.clientX,
                        startY: e.clientY,
                        origin: { x: c.x, y: c.y, w: c.w, h: c.h },
                        now: { x: c.x, y: c.y, w: c.w, h: c.h },
                      });
                    }}
                    onPointerMove={(e) => onMove(e, height)}
                    onPointerUp={onUp}
                    onPointerCancel={() => setDragState(null)}
                  >
                    {c.kind === "line" ? (
                      <svg width="100%" height="100%" aria-hidden="true">
                        {c.orientation === "vertical" ? (
                          <line
                            x1="50%"
                            y1="0"
                            x2="50%"
                            y2="100%"
                            stroke="#000"
                            strokeWidth={c.style?.borderWidth ?? 1}
                          />
                        ) : (
                          <line
                            x1="0"
                            y1="50%"
                            x2="100%"
                            y2="50%"
                            stroke="#000"
                            strokeWidth={c.style?.borderWidth ?? 1}
                          />
                        )}
                      </svg>
                    ) : (
                      summary(c)
                    )}
                    {selectedId === c.id && (
                      <span className="resize-handle" data-handle="resize" aria-hidden="true" />
                    )}
                  </div>
                );
              })}
            </div>
            <div
              className="report-band-resize"
              aria-hidden="true"
              onPointerDown={(e) => {
                capture(e);
                setDragState({
                  kind: "band",
                  key: entry.key,
                  startY: e.clientY,
                  origin: entry.band.height,
                  now: entry.band.height,
                });
              }}
              onPointerMove={(e) => {
                const d = dragRef.current;
                if (d?.kind === "band" && d.key === entry.key)
                  setDragState({
                    ...d,
                    now: Math.max(minHeight, Math.round(d.origin + e.clientY - d.startY)),
                  });
              }}
              onPointerUp={onUp}
            />
          </div>
        );
      })}
    </div>
  );
}
