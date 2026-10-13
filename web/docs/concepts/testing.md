---
sidebar_position: 3
---

# Testing

_Testing support inside ixtable is planned and not implemented._

This page reserves the documentation structure for project-level tests. The repository already tests the ixtable application itself, but an ixtable project cannot define or run its own test suite yet.

:::warning Work in progress

Testing is a documentation stub. There is no testing workspace in the app.

:::

## Planned scope

Project tests should cover data rules and designed workflows. The exact test model, runner, and result format are still open.

:::note TODO

Define the test file format, fixtures, assertions, runner, result history, and continuous integration interface.

:::

## Repository tests

Contributors can test the ixtable codebase with one command. `cargo test --lib` runs the Rust unit tests, the headless interface tests, and the golden application suites. The interface tests drive the real native interface with no window, and they call the real Rust code.

```shell
cd src-tauri && cargo test --lib
```

The screenshot suite renders the real native app headlessly, drives its controls, and captures checked states:

```shell
cd src-tauri && cargo screenshot
```

Continuous integration runs these suites on Windows, macOS, and Linux. It also runs the PostgreSQL conformance suite and the 500 MB archive test on all three systems. Both are off by default on your machine. Set a database URL to run the PostgreSQL tests, or a flag to run the large archive test. Both kinds of test are ignored by default, so pass `--include-ignored`:

```shell
cd src-tauri && IXTABLE_TEST_POSTGRES_URL='postgresql://user:pass@localhost:5432/db?sslmode=disable' cargo test --lib -- --include-ignored recordstore
cd src-tauri && IXTABLE_HEAVY_TESTS=1 cargo test --lib -- --include-ignored durability_tests::heavy
```

`node scripts/ci/start-postgres.mjs` starts a throwaway local server and prints its URL. The large archive test takes about a minute and needs 1.5 GB of free disk.

These commands test ixtable itself. They do not test a user-created ixtable project.

## Next steps

- [Data view](./data-view)
- [Design view](./design-view)
- [Deployment](./deployment)
