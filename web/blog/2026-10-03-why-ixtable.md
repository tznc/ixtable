---
title: Why ixtable
description: ixtable is a free, open-source desktop builder for relational business apps. This post covers what it does and how the paid Cloud service fits in.
slug: why-ixtable
date: 2026-10-03
canonical: https://ixtable.com/blog/why-ixtable
---

ixtable is a desktop app builder for relational business apps. You model data, design forms and reports, and run the finished app from the same program. The desktop app is free and open source under Apache-2.0.

## The problem it solves

Microsoft Access showed that one capable person can build real business software alone. They model the data, build the screens, write the rules, and hand the result to a team. Most teams that outgrow a spreadsheet today have two options. They can build a full web stack, or they can rent a hosted tool that owns their data.

ixtable keeps the Access workflow and updates the parts that aged badly. It runs on Windows, macOS, and Linux. It stores data in SQLite or in a PostgreSQL database you run. Each app is a single `.ixt` file that you can copy, version, and back up like any other document.

## What the free desktop app includes

The free edition is a complete product, not a trial. You get the full Studio for designing apps and a local Runtime for running them. It has a schema designer, saved queries, and one Design page for forms, reports, and dashboards. Every read runs on DuckDB. Expressions, actions, and triggers encode your business rules.

You can share an app in two ways without paying anything. Send the editable `.ixt` project, or export a locked runtime-only bundle with an optional password. Either way you deliver the files yourself, and you deliver updates the same way.

## What ixtable Cloud adds

Manual delivery stops working once you have more than a few users. You cannot see who opened which version. You cannot revoke access when someone leaves. Every update means sending files again.

ixtable Cloud is the paid service for that stage. You publish a checkpoint, and each invited user signs in to the desktop Runtime and gets a signed bundle made for them. Updates arrive on the next sync. Cloud also keeps archive backups, checkpoint history, and an audit log, and it delivers database credentials in encrypted form.

Cloud is priced per cloud application, with a number of runtime users included. Every plan is billed monthly. The [pricing page](/pricing) lists the current plans.

## What Cloud does not do

Cloud does not host your database. SQLite data stays on each desktop, and PostgreSQL stays on servers you operate. There is no browser version of the runtime and no record sync between desktops. Cloud backs up and versions the `.ixt` archive, but it never runs your queries or serves records to your users.

## Where things stand

The desktop app is under active development on [GitHub](https://github.com/ixtable/ixtable). Signed installers are not published yet. Cloud is in testing before its public launch. [Join the waitlist](/#waitlist) to hear when it opens.
