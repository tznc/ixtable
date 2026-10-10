# Form events

Status: accepted. Implements PRD §17.4 (form events) from Phase 7.

## Context

Access forms run macros on events such as Load, Current, BeforeUpdate and
AfterUpdate. ixtable has no scripts: logic is a declarative action (PRD §17.2)
whose expressions run in TypeScript. Form events need a home that reuses those
actions, runs the same in the Studio preview and in Runtime, and passes the
same role checks as a button.

## Decision

**One action per event.** `DesignForm.events` maps an event name to an action
id: `onLoad`, `onCurrent`, `beforeUpdate` and `afterUpdate`. Rust stores it as
`design::FormEvents` and leaves it out of the JSON when no event is bound.
`design::checks::validate` reports an event bound to a missing action as an
error. The form's properties panel has an **Events** section with one action
picker per event. A query-sourced form never saves, so it shows only the
first two.

**One runner.** `src/runtime/RecordView.tsx` raises the events and
`src/runtime/formEvents.ts` runs them through `runAction`, with the context a
button gets (`record`, `form`, `app`, navigation, confirm and notify) and the
signed-in role's `authorize`. The Studio preview renders the same
`FormRenderer`, so events behave the same there.

| Event | Raised | `record` | On failure |
|---|---|---|---|
| On load | once when a record view of the form opens (detail, edit or create) | the first record shown | the error shows on the form |
| On current | each time the view shows another record; a new record in create mode counts | that record | the error shows on the form |
| Before update | after field and form validation, before a create or a changed record is written (saving an unchanged record raises no update events) | the values about to be written | the save is vetoed and the action's error shows |
| After update | after the write committed | the saved values, with generated keys | "Saved, but the after update action failed: …"; the save stays |

Switching between detail and edit mode, or reloading after a save, stays on
the same record, so it raises nothing. On load runs before on current when
both fire. A failing on load action skips on current for that load.

A before update action vetoes the save when it ends with `ok: false`: a
`fail` step (the "explicit cancel step"), a failing step, a declined confirm
("Save cancelled."), or a role that cannot run the action. The action does not
change the values being saved. Record writes it makes commit on their own
unless the action rolls back on error, so a veto after a write leaves that
write in place under `stop`.

An after update action that navigates keeps the form from switching back to
detail mode over the page it opened. Its `refresh` is a no-op because the form
reloads after a save anyway.

## Consequences

- List mode raises no events. Continuous forms (PRD Phase 6) will raise on
  current per row when they land.
- There are no before insert or after insert events: before and after update
  also cover creates, as Access BeforeUpdate and AfterUpdate do.
- Adding `Form.events` moved the config version to 5
  ([archive format](./archive-format.md)).
- Access import does not yet map form event macros to these events.

## Evidence

- `src-tauri/src/design/tests.rs`: `form_events_round_trip_and_must_name_existing_actions`.
- `tests/unit/form-events.test.ts`: event order, veto and after update
  messages, permission denial, design upgrade.
- `tests/integration/form-events.test.tsx`: binds all four events in the
  Studio and runs them in Runtime, including a vetoed save.
