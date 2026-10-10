import type { Sort } from "../../lib/types";

/** Clicking a header sorts by it; Shift-click adds it as the next sort key (or flips it). */
export function nextSorts(sorts: Sort[], column: string, additive: boolean): Sort[] {
  const at = sorts.findIndex((s) => s.column === column);
  const flipped = { column, descending: at >= 0 ? !sorts[at].descending : false };
  if (!additive) return [{ column, descending: at === 0 ? !sorts[0].descending : false }];
  return at >= 0 ? sorts.map((s, i) => (i === at ? flipped : s)) : [...sorts, flipped];
}
