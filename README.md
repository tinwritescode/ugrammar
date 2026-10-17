# ugrammar

Menu-bar grammar fixer for macOS. Select text anywhere, press ⌥⌘G, review the diff, Accept.

- `bun install && bun run dev` — builds the Swift helper (`helper/`), bundles the app, launches Electron.
- `bun test` / `bun run typecheck`
- Engine: any OpenAI-compatible endpoint (default `http://localhost:20128/v1`, model `cx/gpt-6-luna`).
- Settings: `~/Library/Application Support/ugrammar/settings.json`; API key in Keychain.
- See `TESTING.md`, `SPIKE.md`; design decisions in `~/grammar-app-decisions.md`.

Layout: `src/main` (Electron main, engine, settings, flow), `src/renderer` (popover, settings UI), `helper/` (Swift: hotkey, AX, clipboard, Keychain; JSON lines over stdio).

Not in MVP: live check, inline underline, tones, Vietnamese, notarization.

## Build the app

```bash
bun run package   # -> release/ugrammar-darwin-arm64/ugrammar.app (arm64, ad-hoc signed)
```

Ad-hoc signed, not notarized: first open via right-click → Open, then grant Accessibility to ugrammar.
