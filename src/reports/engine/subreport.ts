import type { Band, ReportComponent, SubreportComponent } from "../types";
import type { Row } from "./document";

const EPS = 1e-6;

/** Subreports nest up to three levels below the report that is run (PRD Phase 7). */
export const MAX_SUBREPORT_DEPTH = 3;

export const SUBREPORT_PAGE_BAND = "Subreports are not supported in page headers or footers";
export const SUBREPORT_EXTRA = "A band prints only its first subreport";
export const SUBREPORT_MISSING = "Subreport data is not loaded";
export const SUBREPORT_LOOP = "The subreport prints a report that contains it";
export const SUBREPORT_DEPTH = `Subreports nest at most ${MAX_SUBREPORT_DEPTH} levels deep`;

export const isSubreport = (c: ReportComponent): c is SubreportComponent => c.kind === "subreport";

/**
 * Splits a band around its subreport. The head holds every other component
 * that starts above the subreport's bottom edge and ends at the subreport's
 * top, so components beside it print with the head. The tail holds the
 * components below it, moved up to start at 0. Other subreports are dropped.
 */
export function splitAtSubreport(band: Band, sub: SubreportComponent): { head: Band; tail: Band } {
  const bottom = sub.y + sub.h;
  const rest = band.components.filter((c) => !isSubreport(c));
  const head: Band = {
    height: sub.y,
    keepTogether: band.keepTogether,
    pageBreakBefore: band.pageBreakBefore,
    components: rest.filter((c) => c.y < bottom - EPS),
  };
  const tail: Band = {
    height: Math.max(0, band.height - bottom),
    keepTogether: band.keepTogether,
    pageBreakAfter: band.pageBreakAfter,
    components: rest.filter((c) => c.y >= bottom - EPS).map((c) => ({ ...c, y: c.y - bottom })),
  };
  return { head, tail };
}

// Link values match across number and text (1 = '1'); null never matches.
const sameKey = (a: unknown, b: unknown) =>
  a !== null && a !== undefined && b !== null && b !== undefined && String(a) === String(b);

/** The subreport's rows linked to `parent`: all of them when it has no links. */
export const linkedRows = (sub: SubreportComponent, rows: Row[], parent: unknown): Row[] => {
  const links = sub.links.filter((l) => l.child.trim() && l.master.trim());
  if (!links.length) return rows;
  const record = (parent ?? {}) as Row;
  return rows.filter((row) => links.every((l) => sameKey(row[l.child], record[l.master])));
};
