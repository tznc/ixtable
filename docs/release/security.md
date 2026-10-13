# Webview content security policy

Any script that runs in the webview can call every Tauri command, cloud key
grants included. The CSP in `src-tauri/tauri.conf.json`
(`app.security.csp`) limits what can run there to the app's own bundle.

| Directive | Value | Why |
|---|---|---|
| `default-src` | `'self'` | anything not listed below comes only from the app |
| `script-src` | `'self'` | bundled scripts only: no inline scripts, no `eval`, no CDN |
| `style-src` | `'self' 'unsafe-inline'` | React `style` props, React Flow, and Monaco insert styles at runtime |
| `img-src` | `'self' data: blob:` | attachments and report assets are shown as `data:` URLs |
| `font-src` | `'self' data:` | the Monaco codicon font |
| `connect-src` | `'self' ipc: http://ipc.localhost http://127.0.0.1:54321` | Tauri IPC (macOS/Linux `ipc:`, Windows `http://ipc.localhost`) and the local Supabase stack used by debug builds |
| `worker-src` | `'self' blob:` | the Monaco editor worker, bundled by Vite |
| `object-src` | `'none'` | no plugins |
| `base-uri` | `'self'` | no `<base>` hijacking |
| `form-action` | `'none'` | the app never posts forms |

`dangerousDisableAssetCspModification: ["style-src"]` stops Tauri from adding
a nonce to `style-src`. A nonce would make browsers ignore `'unsafe-inline'`
and break runtime styles. Tauri still hashes inline scripts into `script-src`.

`devCsp` (`node scripts/tauri.mjs dev`) is the same, plus the former Vite dev
server origins (`http://127.0.0.1:1420`, `ws://127.0.0.1:1420`). Tauri hosts
no webview now, so the CSP is kept only for the config.

## Release builds

The release workflow merges an override into the config
(`scripts/release/signing.mjs`, `tauri build --config`). The override replaces
`connect-src` with `'self' ipc: http://ipc.localhost`, plus the origin of
`IXTABLE_CLOUD_BUILD_URL` when that repository variable is set. That URL must
be `https://`. Release builds therefore cannot reach the local Supabase stack,
and they reach only the one production cloud origin. All other cloud traffic
(archive transfers, key grants) goes through Rust, which the CSP does not
govern.

## Monaco

`@monaco-editor/react` loads Monaco from `cdn.jsdelivr.net` by default.
`src/query/monaco-local.ts` (imported by `src/main.tsx`) switches it to the
bundled `monaco-editor` package, with the SQL language and the editor worker
built by Vite. That keeps `script-src 'self'` and makes the editor work
offline.

## Evidence

- `scripts/tests/release-scripts.test.mjs` ("content security policy") checks
  that `default-src` and `script-src` are `'self'`, that neither policy has
  `unsafe-eval`, wildcards, or remote origins, and how release builds rewrite
  `connect-src`.
- The UI is native Rust and Tauri hosts no webview, so the app loads no
  script at runtime. The CSP guards only the bundled config.
