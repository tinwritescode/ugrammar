# Spike: AX vs clipboard fallback

Helper reads via `AXSelectedText` of the frontmost app's focused element; if empty/unavailable it simulates ⌘C (pasteboard saved and restored). Writes via setting `AXSelectedText` (verified), else ⌘V paste (pasteboard saved and restored).

Method is visible with `PROOFY_DEBUG=1 bun run dev` → `/tmp/proofy-debug.log` (app, method, error codes; never text).

| App | Read | Write | Status |
|---|---|---|---|
| TextEdit | AX | AX | Verified end-to-end (hotkey → popover → ⌘↩ → text replaced) |
| Notes | AX/clipboard (not logged on that run) | replaced correctly | Verified end-to-end once |
| Google Chrome (textarea) | **clipboard** (AX returns no selection) | clipboard paste | Read verified. Write-back: first attempt failed because the popover held key focus (since fixed: popover is `focusable:false`, focus lookup is app-scoped). **Re-test by hand.** |
| Slack / VS Code / other Electron | expected clipboard | expected clipboard | Not tested |
| Password fields (AXSecureTextField) | skipped | skipped | Logic only, not hand-tested |

## Findings

- Chromium-based apps do not expose selected text over AX unless accessibility is enabled for them, so the clipboard path is the normal path there.
- Looking up focus system-wide returned the popover once it was key; always scope to the target app's pid, and keep the popover non-focusable so ⌘V goes to the target.
- Write is refused (`focus_changed`) if the frontmost pid differs from the one at read time.
- Accessibility permission attaches to the process that launched Electron (Terminal / Claude / Electron.app in dev). If `status.trusted` is false, the menu bar shows "ug?".
- The gateway at localhost:20128 streams SSE even when not asked and returns empty content for `stream:false` with a system prompt; it ignores plain "reply with corrected text only" and adds commentary. The engine therefore requests `<corrected>` tags and parses both SSE and JSON. Typical latency ≈ 6 s against the 10 s timeout.
