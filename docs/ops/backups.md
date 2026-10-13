# Backups and restore

Two stores hold customer state: the Postgres database (metadata, audit,
billing, envelopes) and the `app-archives` storage bucket (`.ixt` archives).
External customer PostgreSQL datasources are not ixtable data and are never
backed up by ixtable (PRD §23).

## Database

- Enable Point-in-Time Recovery on the production Supabase project
  (Settings → Database → Backups). Target: 7 days PITR minimum, daily
  physical backups kept 30 days.
- Weekly logical dump of the schema and the `audit_events`,
  `billing_events` and `subscriptions` tables to a separate cloud account
  (encrypted bucket, 90-day retention), so a project-level loss does not
  take the audit trail with it.
- Migrations in `supabase/migrations/` are the schema source of truth; a
  restore never re-runs them by hand.

## Archive storage

- The S3 bucket behind Supabase Storage has object versioning on, with a
  lifecycle rule that expires noncurrent versions after 30 days. This
  protects against accidental deletion by `retention-sweep` or
  `account-delete` for 30 days.
- `app_versions.archive_sha256` and `installation_backups.archive_sha256`
  let you verify any restored object. The desktop verifies the hash before
  using bytes, so a corrupted object fails closed.
- Account deletion removes the user's archives. A restore of a deleted
  user's data is refused unless legal requires it; record the decision.

## Secrets

The KEKs (`IXTABLE_KEK_V*`), the bundle signing key and the fingerprint
secret live in the secret manager with versioning on. Losing a KEK makes
every envelope wrapped with it unrecoverable: keep two independent copies
(secret manager + offline escrow under two-person control). Never restore a
database snapshot without the KEK versions it references.

## Restore drill (quarterly)

1. Restore the latest PITR point into a scratch project.
2. Run `npm run service-qa:contracts --prefix web` against a local stack seeded from the
   scratch dump (never against production).
3. Pick five `app_versions` rows, download the objects from the versioned
   bucket, and compare `sha256` with `archive_sha256`.
4. Record time to restore and any gaps in the incident log.
