# Manual test checklist

Setup: `bun install && bun run dev`. Menu bar shows **Pf**. First run: privacy dialog → OK. Grant Accessibility to the app that launched it (System Settings → Privacy & Security → Accessibility) when prompted, then restart. Optional: `PROOFY_DEBUG=1 bun run dev` and `tail -f /tmp/proofy-debug.log`.

Automated: `bun test` (engine, diff, settings, flow) and `bun run typecheck`.

## Core flow
- [ ] Notes: type `She dont knows nothing about the meeting yesterday.`, select it, press ⌥⌘G. Popover near the cursor shows red/green diff within ~10 s. Click **Accept** → text replaced. Notes keeps focus throughout.
- [ ] TextEdit: same; also try ⌘↩ (accept) and Esc (dismiss).
- [ ] Chrome (any textarea, e.g. a GitHub comment box): same. Expect clipboard path; text replaced; your clipboard content is restored afterwards.
- [ ] Dismiss leaves text untouched.
- [ ] Already-correct text → "No changes needed ✓" toast, no change.
- [ ] No selection → "Select some text first."
- [ ] Press hotkey twice quickly: no crash, no double write.

## Privacy
- [ ] Password field (e.g. Safari/Chrome login form): message "Skipped: secure/password field.", nothing sent (check endpoint logs).
- [ ] Terminal / 1Password (default exclusions): "Skipped: this app is on the exclusion list."
- [ ] Menu bar → **Pause**: hotkey does nothing; title shows ⏸. Unpause restores.
- [ ] First launch shows the privacy notice once (delete `firstRunAcknowledged` from settings.json to re-see).
- [ ] No text content in any log / settings.json.

## Errors (original text must never change)
- [ ] Stop the endpoint (or set endpoint to `http://localhost:1/v1`): notification "Check failed… not changed", menu title **Pf!**, status line shows the error.
- [ ] Set a wrong model name: error notification, text unchanged.
- [ ] Slow endpoint (>10 s): timeout error, text unchanged.

## Settings (menu bar → Settings…)
- [ ] Defaults: endpoint `http://localhost:20128/v1`, model `cx/gpt-6-luna`.
- [ ] **Test connection** ✓ with defaults; ✕ with a bad endpoint, message shown.
- [ ] Change model → Save → `~/Library/Application Support/Proofy/settings.json` updated → relaunch keeps it.
- [ ] API key: enter, Save → "(stored in Keychain)"; `grep -i key settings.json` finds no secret; Keychain Access has item "Proofy / api-key".
- [ ] Dictionary: add `Xpiki`; text containing "Xpiki" is not re-cased/changed.
- [ ] Excluded apps: add via **Choose app…**, hotkey in that app is skipped; remove → works again.
- [ ] Hotkey: change to e.g. ⌃⌥K, Save → new combo works, old doesn't. Zero modifiers is rejected (previous kept).
- [ ] Accessibility not granted → status "Needs Accessibility permission", banner in Settings with button.

## Menu bar
- [ ] Loading: right after ⌥⌘G a "Checking…" spinner popover appears, then becomes the diff (or disappears on error).
- [ ] Status line: Ready / Checking… / Paused / Error / Needs Accessibility permission.
- [ ] Quit stops the helper (`pgrep grammar-helper` empty).

## Typing badge
- [ ] Click into a TextEdit/Notes text area: a small green dot appears beside the caret and pulses while you type/move.
- [ ] In Chrome/Electron apps (no caret bounds) the dot sits in the bottom-right corner of the focused field.
- [ ] Dot is hidden in password fields, excluded apps, when paused, and without Accessibility permission.
- [ ] Press ⌥⌘G: dot turns into a spinner while checking.
