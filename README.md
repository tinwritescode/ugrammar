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
- `src/renderer/` holds the web UI (popover, tray panel, badge, underline), bundled into `dist/` by `scripts/build.ts`.
- `helper/` is the Swift sidecar: hotkey, Accessibility read/write, clipboard, Keychain. It speaks JSON lines over stdio.

## Build the app

```bash
bun run package   # -> release/ugrammar.zip (ugrammar.app, arm64, ad-hoc signed)
```

Ad-hoc signed, not notarized. Open it the first time with right-click → Open, then grant Accessibility to ugrammar.
