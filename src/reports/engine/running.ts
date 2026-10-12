import type { Band, CalculatedComponent, FieldComponent, ReportGroup } from "../types";
import { accumulate, type RunningValue } from "./values";

type RunningComponent = FieldComponent | CalculatedComponent;

/** A band instance that can carry running-sum values. */
interface Instance {
  band: Band;
  scope: Record<string, unknown>;
  /** Running-sum values of this instance (detail and group bands). */
  running?: Map<string, RunningValue>;
}

const runningOf = (band: Band) =>
  band.components.filter(
    (c): c is RunningComponent => (c.kind === "field" || c.kind === "calculated") && !!c.runningSum,
  );

/**
 * Running sums of one printed report: totals so far by component id. A
 * "group" sum restarts at each instance of the enclosing group (`restartBelow`).
 */
export function runningSums(now?: Date) {
  const sums = new Map<string, RunningValue>();
  const restart = (band: Band) => {
    for (const c of runningOf(band)) if (c.runningSum === "group") sums.delete(c.id);
  };
  return {
    /** Adds the instance to the sums of its band's running components. */
    counted<T extends Instance>(b: T): T {
      const comps = runningOf(b.band);
      if (!comps.length) return b;
      const running = new Map<string, RunningValue>();
      for (const c of comps) {
        const next = accumulate(sums.get(c.id), c, b.scope, now);
        sums.set(c.id, next);
        running.set(c.id, next);
      }
      return { ...b, running };
    },
    /** A new instance of group `level` starts: bands one level down restart "group" sums. */
    restartBelow(groups: ReportGroup[], level: number, detail: Band) {
      const inner = groups[level + 1];
      if (inner) {
        restart(inner.header);
        restart(inner.footer);
      } else restart(detail);
    },
  };
}
