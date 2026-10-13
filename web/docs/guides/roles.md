---
sidebar_position: 6
---

# Roles and permissions

_Roles decide which navigation items, forms, reports, dashboards, tables, queries, and actions each Runtime user can use. They are not a database security boundary._

A **role** is a named set of grants. Each grant names one object and what the role can do with it. A role starts with no grants, and anything not granted is denied. You define roles in Studio, preview them in the Runtime, and ixtable Cloud assigns one to each user of a cloud application.

Open Settings, then the **Roles** tab. Choose **New role**, give it a name, and grant access in three lists.

## Objects

The **Objects** table lists every form, report, dashboard, table, and saved query, with **Read**, **Create**, **Update**, and **Delete** columns.

- Turning on create, update, or delete also turns on read. Turning off read clears the others.
- Form permissions govern records changed through that form.
- Table permissions govern table pages, generated forms, and actions that write to the table.
- Reports and dashboards use read only.

A grant on a form or report also lets the role read the tables and queries it needs to load, such as the source of a lookup.

Read on a form, report, or dashboard also lets the role read the saved queries it shows. For a dashboard, that covers its numbers, charts, tables, and filter choices. It does not cover embedded forms and reports. Each of those needs its own grant, and each button needs its action.

Permissions apply to whole objects. Dashboard filters and columns do not limit which rows or columns a role can read. To show a role less, write a narrower saved query and grant that one.

## Navigation

The **Navigation** list has one entry per navigation item, such as "Show Orders in navigation". A role sees an item only when it is listed here and the role can read the object it opens. A page that a role cannot read says "You do not have access to this page." even when an action opens it.

## Actions

The **Actions** list grants the right to run each action. A button whose action is not granted stays disabled. While an action runs, each step also needs the role's permission on its table, query, or nested action.

Triggers have their own setting, **Run as**. A trigger that runs as the app can write tables the role cannot. A trigger that runs as the signed-in user needs the role to hold every permission the trigger's action uses. When the role lacks one, ixtable refuses the save that would fire it, and the Roles tab shows a warning for that role. [Automation](./automation#run-as) describes both modes.

## Preview a role

In Studio, open the menu on the **Run App** button and pick a role. The app opens in its own window, and its navigation, forms, buttons, and dashboards behave as they will for that role. The record commands refuse anything the role cannot do. Switch roles with **Run as** in that window. **Owner (default)** runs with full access. The role applies only while the Run App window is in front, so Studio keeps full access, and closing the window ends the preview.

## Who gets which role

| Where the application runs | Role |
| --- | --- |
| Studio design modes | Developer, full access |
| Studio Run App window | Owner, or the role picked in **Run as** |
| Manually distributed runtime bundle | Developer, full access. Roles do not apply |
| ixtable Cloud installation | The role assigned to the user, shown as `Role: <name>` |

A cloud user without an assigned role gets no access. A role id that no longer exists denies everything.

Roles do not restrict a manually distributed bundle. Use a separate bundle, or ixtable Cloud, when users need different access. Field-level and row-level permissions do not exist.

## Where ixtable enforces roles

The Runtime checks roles in the interface and again in the desktop app's core. The interface hides menu items, buttons, and pages the role cannot use. The core checks each record read, record write, saved query run, and report export against the current role, so a request that bypasses the interface is refused too. Ad hoc SQL, checkpoints, and resetting installation data are never available to a role.

## PostgreSQL limitation

Roles control what the ixtable Runtime shows and allows: navigation, forms, reports, dashboards, queries, and actions. They are not a database security boundary.

When an application connects straight to PostgreSQL, each Runtime user's computer holds credentials that can reach the database. A user who extracts those credentials can connect with another client and read or change anything the database account allows, whatever their ixtable role says. ixtable roles do not defend against a malicious authorized user.

For strong isolation, give each group of users a separate least-privileged PostgreSQL account, and grant that account only the tables and operations its users need. Set up those accounts and permissions in PostgreSQL yourself. The Roles tab shows this limitation at the top, and [Cloud security](../cloud/security) describes the trust model for cloud applications.

## Next steps

- [Runtime forms](./runtime-forms)
- [Automation](./automation)
- [Runtime bundles](./runtime-bundles)
- [Cloud security](../cloud/security)
