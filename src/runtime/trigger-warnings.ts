import type { Step } from "../automation/types";
import type { DocumentConfig } from "../lib/types";
import { ACTION_QUERY_OPS } from "../query/types";
import { can } from "./rbac";
import type { Operation } from "./types";

type Needed = { kind: string; id: string; op: Operation };

/** The action a `customAction` entity routes updates and deletes of `table` to. */
const routedTo = (config: Pick<DocumentConfig, "entities">, table: string) => {
  const entity = (config.entities ?? []).find((e) => e.table === table);
  return entity?.concurrency === "customAction" ? entity.actionId : undefined;
};

/**
 * Permissions a trigger's action needs, following branches, runAction calls and
 * the custom actions its updates and deletes are routed to (trigger_auth.rs).
 */
function needed(
  config: Pick<DocumentConfig, "actions" | "entities" | "savedQueries">,
  actionId: string,
): Needed[] {
  const out: Needed[] = [{ kind: "action", id: actionId, op: "execute" }];
  const seen = new Set<string>();
  const queue = [actionId];
  while (queue.length) {
    const id = queue.pop() as string;
    if (seen.has(id)) continue;
    seen.add(id);
    const steps: Step[] = [...((config.actions ?? []).find((a) => a.id === id)?.steps ?? [])];
    while (steps.length) {
      const step = steps.pop() as Step;
      if (step.kind === "condition") steps.push(...(step.then ?? []), ...(step.else ?? []));
      if (step.kind === "createRecord") out.push({ kind: "table", id: step.table, op: "create" });
      if (step.kind === "updateRecord") out.push({ kind: "table", id: step.table, op: "update" });
      if (step.kind === "deleteRecord") out.push({ kind: "table", id: step.table, op: "delete" });
      if (step.kind === "runQuery") {
        const action = config.savedQueries?.find((q) => q.id === step.queryId)?.action;
        // An action query needs the table operations it performs, not query read.
        if (action)
          for (const op of ACTION_QUERY_OPS[action.kind])
            out.push({ kind: "table", id: action.table, op });
        else out.push({ kind: "query", id: step.queryId, op: "read" });
      }
      if (step.kind === "runAction") {
        out.push({ kind: "action", id: step.actionId, op: "execute" });
        queue.push(step.actionId);
      }
      const custom =
        (step.kind === "updateRecord" || step.kind === "deleteRecord") &&
        routedTo(config, step.table);
      if (custom) {
        out.push({ kind: "action", id: custom, op: "execute" });
        queue.push(custom);
      }
    }
  }
  return out;
}

/** `op kind "name"`, naming actions and queries by their display name (ids are uuids). */
function describe(config: Pick<DocumentConfig, "actions" | "savedQueries">, n: Needed): string {
  const name =
    n.kind === "action"
      ? config.actions?.find((a) => a.id === n.id)?.name
      : n.kind === "query"
        ? config.savedQueries?.find((q) => q.id === n.id)?.name
        : undefined;
  return `${n.op} ${n.kind} "${name ?? n.id}"`;
}

export interface TriggerGap {
  triggerId: string;
  name: string;
  missing: string[];
}

/**
 * Enabled "run as signed-in user" triggers whose steps `roleId` cannot perform
 * (Rust refuses the initiating save for that role). Explicit grants only, so the
 * warning may also list access a form grant would imply.
 */
export function userTriggerGaps(
  config: Pick<DocumentConfig, "roles" | "actions" | "triggers" | "entities" | "savedQueries">,
  roleId: string,
): TriggerGap[] {
  return (config.triggers ?? [])
    .filter((t) => t.enabled && t.runAs === "user")
    .map((t) => {
      const missing = needed(config, t.actionId)
        .filter((n) => !can(config, roleId, n.kind, n.id, n.op))
        .map((n) => describe(config, n));
      return { triggerId: t.id, name: t.name, missing: [...new Set(missing)] };
    })
    .filter((gap) => gap.missing.length > 0);
}
