# ugrammar

Menu-bar grammar fixer for macOS. Select text anywhere, press ⌥⌘G, review the diff, Accept. With auto-check on, it also checks each sentence as you finish typing it.

- `bun install && bun run dev` builds the Swift helper (`helper/`), bundles the web UI, and launches the Tauri app.
- `bun run test` runs the Rust tests (`core/` and `src-tauri/`). `bun run typecheck` checks the UI.
- Engine: any OpenAI-compatible endpoint (default `http://localhost:20128/v1`, model `xpiki/gpt-6.1-sol`).
- Settings: `~/Library/Application Support/ugrammar/settings.json`. The API key lives in the Keychain.
- Set `PROOFY_DEBUG=1` to log outcomes (never text) to `/tmp/proofy-debug.log`.
- See `TESTING.md`, `SPIKE.md`; design decisions in `~/grammar-app-decisions.md`.

Layout:

- `core/` is the platform-free Rust crate: settings, engine client, diff, sentence and span logic, insights.
- `src-tauri/` is the app shell: windows and panels, the tray, the hotkey/check flow, auto-check, and the helper process.
- `src/renderer/` holds the web UI (popover, tray panel), bundled into `dist/` by `scripts/build.ts`.
- `helper/` is the Swift sidecar: hotkey, Accessibility read/write, clipboard, Keychain. It speaks JSON lines over stdio.

## Build the app

```bash
bun run package   # -> release/ugrammar.zip (ugrammar.app, arm64, ad-hoc signed)
bun run release   # -> release/: signed app zips, ugrammar.app.tar.gz, its .sig, latest.json
```

Not notarized. Open it the first time with right-click → Open, then grant Accessibility to ugrammar.

`bun run release` signs with the self-signed certificate in `~/.ugrammar-signing` (`key.pem`, `cert.pem`) through `rcodesign` (`cargo install apple-codesign`), and signs the updater archive with `updater.key`. Accessibility grants follow the certificate, so they survive updates; an ad-hoc build loses them. Upload everything in `release/` to the GitHub release tagged `v<version>`. Installed apps read `latest.json` from the latest release every six hours, install the update in place, and offer a restart from the menu bar panel. Back up `~/.ugrammar-signing`: without those keys, installed apps reject new updates.

## Author

Tín Nguyễn ([@tinwritescode](https://github.com/tinwritescode)). Source and issues: https://github.com/tinwritescode/ugrammar
