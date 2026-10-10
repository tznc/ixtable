---
sidebar_position: 5
---

# Automation

_Actions run a list of steps on demand. Triggers run an action when a record is created or updated, either inside the save or later on a background job queue._

Automation mode has three tabs: **Actions**, **Triggers**, and **Jobs**. An action is a named list of steps, such as creating a record, running a query, or opening a form. A form button, a dashboard button, a trigger, or another action can run it. Steps use the [expression language](../reference/expressions) for values and conditions, so an action never runs code.

When an action or trigger has a problem, such as a missing table or a cycle between actions, a summary of automation problems appears above the tabs. ixtable checks the definitions again a moment after each change.

## Actions

Choose **New action** on the Actions tab, then give the action a name and add steps. Pick a kind in **New step kind** and choose **Add step**. Reorder steps with the arrow buttons. Every step except If / else has an **Only when** expression. When it gives false or null, the action skips the step and logs it as skipped.

| Step | What it does |
| --- | --- |
| Create record | Inserts a row into a table. **Store result as** keeps the new values and generated keys for later steps |
| Update record | Updates the current record, or every row whose columns match the values you give |
| Delete record | Deletes the current record or the matching rows |
| Run query | Runs a saved query with parameters and stores its rows, under `rows` by default |
| Navigate | Opens a form, report, dashboard, or table, with an optional mode and record id |
| Open form | Opens a form in list, detail, create, or edit mode, with an optional record id |
| Open report | Opens a report with parameter values |
| Open dashboard | Opens a dashboard. Its parameters become the dashboard's initial filter values |
| Set state | Sets a key in the application state or the form state. Later steps see the new value |
| Confirm | Asks the user to confirm a message. Declining cancels the action |
| Show message | Shows an information or error message |
| If / else | Runs the **Then** steps when the condition is true, and the **Else** steps otherwise |
| Run action | Runs another action |
| Fail with message | Stops the action with an error message |

Step expressions can read `record`, `old`, `form`, `app`, `params`, and `results`. `results` holds the values that earlier steps stored by name, so a Run query step that stores `rows` makes `results.rows` available. To change a field on the record on screen, use Update record with **The current record**.

## Errors and transactions

**On error** decides what happens when a step fails.

| Setting | Behavior |
| --- | --- |
| Stop at the first failure | The action ends at the failing step. Writes from earlier steps stay saved. This is the default |
| Log failures and continue | The action logs the failure and runs the rest of the steps. Fail with message and a declined Confirm still end it |
| Roll back all record changes | The action collects its writes and commits them at the end as one transaction. Any failure saves nothing |

In rollback mode, steps read the data as it was before the action started, so they do not see their own pending writes. Messages, navigation, and state changes wait until the commit succeeds. A Run action step joins the transaction of the action that called it.

Updates and deletes send the values each step read, so a table with an optimistic concurrency policy rejects the write when someone changed the row in between. Actions can nest up to 10 deep, and a cycle such as `A → B → A` fails.

## Test an action

The Actions tab has a **Test run** section. Enter a sample record as JSON and choose **Test run**. The step log lists each step with its kind, its result, and how long it took. Navigation and state changes appear as text in the results.

A test run writes to the document's real records. It also runs without role checks.

## Run actions from forms and dashboards

A button control on a form runs the action set in its **Button action** property. The step expressions see the record on screen as `record`, the form state as `form`, and the application state as `app`. The button stays disabled while the record loads, while another action runs, and when the current role cannot run the action. A failure shows as a notice on the form.

A dashboard button runs the action picked in its **Action** property. Its steps see the dashboard filters as `params` and the application state as `app`. A dashboard button cannot set form state.

## Form events

A form can run an action when something happens to it. Select the form in Design mode and pick an action for each event in the **Events** section of its properties.

| Event | When it runs |
| --- | --- |
| On load | The form opens on a record, or on a new record |
| On current | The form moves to another record. Switching between viewing and editing the same record does not count |
| Before update | The user saves a new or changed record, after the field checks pass and before the record is written |
| After update | The record was saved |

The steps see the record as `record`, like a button's steps. In before update, `record` holds the values about to be saved. When the action fails, the record is not saved and its message shows on the form. A **Fail with message** step with a condition is the usual way to reject a save, for example `record.end < record.start`. A declined Confirm step also cancels the save.

If an after update action fails, the record stays saved and the form says so. Events check the same role permissions as buttons. A form that reads a saved query cannot save, so it offers only on load and on current. Forms in list mode run no events.

## Triggers

A trigger runs an action when a record in a table is created or updated. Choose **New trigger** on the Triggers tab and set the table, the event, and the action. Deleting a record never fires a trigger.

The optional **Condition** reads the saved row as `record` and the application state as `app`. On an update it also reads the row before the change as `old`, so `record.status != old.status` fires only when the status changes. A condition that gives false or null skips the trigger. Clear **Enabled** to turn a trigger off without deleting it.

Every record write in ixtable fires triggers, whether it comes from a form, the records grid, a dashboard, or an action. A trigger whose action writes to the same table can fire itself again. ixtable stops the chain after 5 levels with an error.

## Sync and async triggers

**Run** picks when the action runs.

A **synchronous** trigger runs right after the record is saved, as part of the same operation. The person who saved the record waits for it. When its action fails, they see the error, but the save stays. A synchronous trigger can open forms and show messages in the Runtime.

An **asynchronous** trigger adds a job to the background queue and returns at once. The job runs a moment later, with retries. Because nobody is waiting on it, it cannot navigate, ask for confirmation, or set form state. Its messages go to the attempt log.

## Run as

**Run as** decides whose permissions the trigger's action uses.

- **App** is the default. The trigger can write to tables that the user's role cannot, but only the tables and operations its action contains.
- **Signed-in user's role** checks the user's role before the save. When the role lacks a permission the trigger needs, ixtable refuses the save and names the missing permissions. The Roles tab warns about these triggers.

Run query steps still need the role's read access in either mode. See [Roles and permissions](./roles) for how roles grant access to actions and tables.

## Background jobs

An async trigger has three more settings. **Max attempts** defaults to 3. **Retry backoff (ms)** defaults to 1000 and doubles after each failed attempt, up to one hour. The **Idempotency key** expression decides which jobs count as the same job.

By default the key combines the trigger, table, record key, event, a hash of the values, and the save that caused it. A retry of the same save does not queue a second job, but a later identical save does. Write your own key when a job must run once per record, for example `trigger.id & ':' & record.id`.

The queue lives on your computer and survives restarts. Jobs run only while ixtable has the document open. A job that was running when ixtable closed runs again when you reopen the document. A job can therefore run more than once, so write actions that are safe to repeat.

## Jobs tab

The Jobs tab lists the newest 200 jobs and refreshes every two seconds. Filter by **Status**: `queued`, `running`, `succeeded`, `failed`, or `cancelled`.

| Column | Shows |
| --- | --- |
| Trigger | The trigger that queued the job |
| Action | The action the job runs |
| Status | Where the job is in the queue |
| Attempts | Attempts used out of the maximum |
| Next run | When a queued job runs next |
| Last error | The error from the latest failed attempt |

**History** shows every attempt with its step log. **Cancel** stops a queued job from running. Cancelling a running job discards its result, but records it already wrote stay saved. **Retry** queues a failed or cancelled job to run now.

A disabled or deleted trigger does not cancel jobs it already queued. In a runtime-only bundle, the same queue and history open from **Diagnostics…** in the sidebar.

## Limits

Automation runs only on the computer that has the application open. There are no schedules, webhooks, or cloud workers, and triggers fire only on create and update.

## Next steps

- [Expressions](../reference/expressions)
- [Runtime forms](./runtime-forms)
- [Roles and permissions](./roles)
- [Queries](./queries)
