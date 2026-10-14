---
sidebar_position: 8
---

# Runtime bundles

_A runtime bundle is a signed `.ixtr` file that holds a complete application. Recipients run it without Studio and install updates by opening a newer file._

You export a bundle from Studio and send the file yourself, by email or a shared drive. The recipient opens it in ixtable, which checks the signature and installs the application in a window without Studio's design tools. Each recipient keeps their own records, and later bundles update the design without replacing those records. No account or network connection is involved. [Deployment](../concepts/deployment) summarizes the options, and [ixtable Cloud](../cloud/getting-started) covers private distribution with automatic updates.

## What a bundle contains

A bundle wraps a complete project file: the definitions, the [migrations](./migrations), the application assets, and a snapshot of the current records. The snapshot seeds each recipient's records the first time they open the bundle. A header outside the project file names the application, its version, the release notes, the minimum Runtime version, and the signer's public key.

Every bundle carries the project's id. ixtable treats bundles with the same id as versions of the same application.

## Export a bundle

Open Settings, then the **Release** tab.

| Field | Meaning |
| --- | --- |
| Version | A semantic version such as `1.2.0`. Each update needs a higher version than the last |
| Minimum Runtime version | Optional. Older versions of ixtable refuse to open the bundle |
| Release notes | Shown to recipients before they install an update |

Choose **Export runtime bundle…** and pick where to save the file. ixtable saves the project first. A validation error in the project, including a migration problem, blocks the export until you fix it.

After the export, the Release tab shows the file, its size, its SHA-256 hash, the signer fingerprint, and whether it has a password.

## Signing

ixtable signs every bundle with an Ed25519 key. It creates the key on your computer the first time you export and keeps it in its state folder with permissions that only your user account can read. The key is a local developer key. It is not tied to an ixtable Cloud account.

Recipients pin your key the first time they open a bundle for an application. From then on, a bundle for that application signed by any other key does not open. The first bundle has no earlier key to compare against, so send it, or its signer fingerprint, over a channel the recipient trusts.

ixtable has no way to rotate the key. If you lose it, existing installations refuse every bundle signed by a new key, so back up the key file.

## Password protection

Select **Protect the bundle with a password** and enter a password of at least 8 characters twice. ixtable derives a key from the password with Argon2id and encrypts the project inside the bundle with XChaCha20-Poly1305. The header stays readable, so ixtable can show the name and version before asking for the password. Share the password separately from the file.

A password protects the bundle at rest and against casual unauthorized opening. It does not stop an authorized recipient from extracting the data the application shows them, or any credentials the application can reach. After installation, the recipient's copy is stored unencrypted and protected only by their account's file permissions.

## Open a bundle

The recipient chooses **Open runtime bundle…** on the start screen, or double-clicks the `.ixtr` file. For a protected bundle, ixtable asks for the **Bundle password** before it opens anything.

ixtable checks the file before it reads any of the project:

1. The file is at most 600 MB.
2. The signature matches every byte of the file. A changed or damaged file does not open.
3. The signer matches the key pinned for this application.
4. This version of ixtable meets the bundle's minimum Runtime version.
5. The password decrypts the project.
6. The project's checksum matches the header.

The first bundle for an application installs and opens right away. Opening the same version again opens the existing installation unchanged.

## The Runtime window

A bundle opens in a window titled with the application name. It shows the application's navigation and [forms](./runtime-forms), with no design modes and no save buttons. The sidebar shows the version and three buttons:

- **Check for update…** installs a newer or older bundle file.
- **Reset installation data…** replaces the recipient's records with the records in the bundle.
- **Diagnostics…** shows the background job queue and the local log.

A manually distributed bundle runs with full access to the application. [Roles](./roles) apply to cloud installations and to the role preview in Studio.

## Install an update

To update, the recipient chooses **Check for update…** and picks the new file. ixtable checks it the same way as a first open. It refuses a file for a different application, or the version that is already installed.

A confirm dialog shows the installed version, the release notes, and each migration that will run on the recipient's records, with its SQL. **Apply update** then runs these steps:

1. Copy the current installation to a recovery folder.
2. Stage the new definition with a copy of the recipient's records.
3. Run the pending migrations on the copy.
4. Check database integrity and foreign keys, and confirm that every table and column the forms use exists.
5. Swap the staged copy in.

Any failure leaves the previous version running with its records unchanged. A successful update reports that the records were kept.

An older version never installs. ixtable says which newer version is already installed, because the recipient's records may already use the newer version's tables.

## Reset installation data

**Reset installation data…** replaces every record in the installation with the records bundled with the installed version. The dialog shows the number of records in each table now and after the reset. **Replace all records** keeps a recovery copy of the current data in the installation folder before it swaps the bundled records in.

## Where installations live

ixtable keeps each installation in its state folder, under `data/installations/<application id>`. The folder holds the active definition, the recipient's records, and the recovery copy from the last update. ixtable has no uninstall command.

| Platform | State folder |
| --- | --- |
| Windows | `%LOCALAPPDATA%\ixtable` |
| macOS | `~/Library/Application Support/ixtable` |
| Linux | `~/.local/share/ixtable` |

## PostgreSQL applications

A bundle never contains a PostgreSQL password. The project file stores only a reference to a password saved on your computer. An installed PostgreSQL application reads and writes the database it is configured for, and its migrations do not change that database's schema.

## Next steps

- [Deployment](../concepts/deployment)
- [Migrations](./migrations)
- [Roles and permissions](./roles)
- [ixtable Cloud](../cloud/getting-started)
