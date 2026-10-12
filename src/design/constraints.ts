import type { SpanConstraints } from "../grid/types";
import type { ControlKind } from "./schema";

/** Smallest useful width in columns per control kind, authored against a 12-column grid. */
const MINIMUMS: Record<ControlKind, number> = {
  label: 1,
  text: 2,
  multiline: 3,
  number: 2,
  decimal: 2,
  boolean: 1,
  date: 2,
  time: 2,
  datetime: 3,
  select: 2,
  relationship: 3,
  computed: 2,
  button: 1,
  section: 4,
  tabs: 4,
  relatedList: 6,
  image: 2,
  richText: 4,
  attachment: 3,
  multiSelect: 3,
};

/**
 * Resize limits for a control on a grid with `columns` columns. Minimums scale with the
 * column count (never below 1, never above the grid), and the maximum is the full width.
 */
export function controlConstraints(kind: ControlKind, columns: number): SpanConstraints {
  const count = Math.max(1, columns);
  const min = MINIMUMS[kind] ?? 1;
  const minColumnSpan = Math.min(count, Math.max(1, Math.round((min * count) / 12)));
  return { minColumnSpan, maxColumnSpan: count };
}
