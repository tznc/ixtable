import { newId } from "../lib/utils";
import type { Step, StepKind } from "./types";

export const STEP_LABELS: Record<StepKind, string> = {
  createRecord: "Create record",
  updateRecord: "Update record",
  deleteRecord: "Delete record",
  runQuery: "Run query",
  navigate: "Navigate",
  openForm: "Open form",
  closeForm: "Close popup form",
  openReport: "Open report",
  openDashboard: "Open dashboard",
  setState: "Set state",
  confirm: "Confirm",
  message: "Show message",
  condition: "If / else",
  runAction: "Run action",
  fail: "Fail with message",
};

export function newStep(kind: StepKind): Step {
  const id = newId();
  switch (kind) {
    case "createRecord":
      return { id, kind, table: "", values: {} };
    case "updateRecord":
      return { id, kind, table: "", match: "current", values: {} };
    case "deleteRecord":
      return { id, kind, table: "", match: "current" };
    case "runQuery":
      return { id, kind, queryId: "", params: {}, storeAs: "rows" };
    case "navigate":
      return { id, kind, target: { kind: "form", id: "" } };
    case "openForm":
      return { id, kind, formId: "" };
    case "closeForm":
      return { id, kind };
    case "openReport":
      return { id, kind, reportId: "" };
    case "openDashboard":
      return { id, kind, dashboardId: "" };
    case "setState":
      return { id, kind, scope: "app", key: "", value: "" };
    case "confirm":
      return { id, kind, message: "'Are you sure?'" };
    case "message":
      return { id, kind, text: "", tone: "info" };
    case "condition":
      return { id, kind, when: "", then: [], else: [] };
    case "runAction":
      return { id, kind, actionId: "" };
    case "fail":
      return { id, kind, message: "" };
  }
}
