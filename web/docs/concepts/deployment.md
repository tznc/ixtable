---
sidebar_position: 4
---

# Deployment

_Deploy an ixtable application by exporting a signed runtime-only bundle and sending it to the people who use it._

A **runtime-only bundle** is an `.ixtr` file that holds a complete application. Recipients open it in ixtable and can run the application but not edit its design. You distribute the file yourself, by email or a shared drive. To distribute privately with automatic updates instead, see [ixtable Cloud](../cloud/getting-started).

## Export a bundle

Open Settings, then the Release tab. Enter a version such as `1.2.0`, release notes, and an optional minimum Runtime version, then choose **Export runtime bundle…**. ixtable signs every bundle with a key it creates on your computer the first time you export. After an export, the Release tab shows the signer fingerprint. Send it to recipients over a channel they trust.

ixtable refuses to export while the application has problems. That includes any formula with an error, such as a visibility condition with a typo. Settings › Problems lists each one with an Open link that takes you to the field. Publishing to ixtable Cloud is blocked the same way.

You can also protect the bundle with a password. A password protects the bundle at rest and against casual unauthorized opening. It does not stop an authorized recipient from extracting the data the application shows them, or any credentials the application can reach.

## Open and update a bundle

Recipients choose **Open runtime bundle…** on the start screen. ixtable checks the signature before it reads anything else, so a changed or damaged file does not open. The first bundle for an application also pins your signing key. A later bundle for that application signed by any other key is refused.

Each recipient keeps their own records. The bundle's records seed the installation on first open only. To install a new version, the recipient chooses **Check for update…** and picks the new file. ixtable applies the new design and its migrations to a copy of the recipient's records. It switches to that copy only after every migration and health check passes. A failed update leaves the previous version running. An older version than the installed one is refused. [Runtime bundles](../guides/runtime-bundles) covers each step.

## Databases

An application that uses the embedded SQLite database gives every recipient a separate copy of the records. An application that uses PostgreSQL connects each recipient to the database you configured. The bundle never contains the PostgreSQL password. The project file only stores a reference to a password saved on your computer.

When a recipient opens a PostgreSQL bundle, the Runtime asks for a database login. It also asks when the database refuses the saved login. The username starts as the one you configured, and the recipient can change it, so you can give each person their own least-privileged role. The Runtime connects before it saves anything. It then keeps the login encrypted on that computer for that installation, never in the bundle. **Database login…** in the Runtime sidebar changes the login or forgets it.

## Next steps

- [Runtime bundles](../guides/runtime-bundles)
- [Migrations](../guides/migrations)
- [Overview](/docs/)
- [Design view](./design-view)
- [Testing](./testing)
