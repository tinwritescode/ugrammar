# Manual test checklist

Setup: `bun install && bun run dev`. Menu bar shows **ug**. First run: privacy dialog → OK. Grant Accessibility to the app that launched it (System Settings → Privacy & Security → Accessibility) when prompted, then restart. Optional: `PROOFY_DEBUG=1 bun run dev` and `tail -f /tmp/proofy-debug.log`.

Automated: `bun test` (engine, diff, settings, flow) and `bun run typecheck`.

## Core flow
- [ ] Notes: type `She dont knows nothing about the meeting yesterday.`, select it, press ⌥⌘G. Popover near the cursor shows red/green diff within ~10 s. Click **Accept** → text replaced. Notes keeps focus throughout.
- [ ] TextEdit: same; also try ⌘↩ (accept) and Esc (dismiss).
- [ ] Chrome (any textarea, e.g. a GitHub comment box): same. Expect clipboard path; text replaced; your clipboard content is restored afterwards.
- [ ] Dismiss leaves text untouched.
- [ ] Already-correct text → "Không có lỗi nào ✓" toast, no change.
- [ ] No selection → "Hãy chọn chữ trước."
- [ ] Press hotkey twice quickly: no crash, no double write.

## Privacy
- [ ] Password field (e.g. Safari/Chrome login form): message "Bỏ qua: ô mật khẩu hoặc ô bảo mật.", nothing sent (check endpoint logs).
- [ ] Terminal / 1Password (default exclusions): "Bỏ qua: app này nằm trong danh sách bỏ qua."
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
- [ ] Change model → Save → `~/Library/Application Support/ugrammar/settings.json` updated → relaunch keeps it.
- [ ] API key: enter, Save → "(stored in Keychain)"; `grep -i key settings.json` finds no secret; Keychain Access has item "Proofy / api-key".
- [ ] Dictionary: add `Xpiki`; text containing "Xpiki" is not re-cased/changed.
- [ ] Excluded apps: add via **Choose app…**, hotkey in that app is skipped; remove → works again.
- [ ] Hotkey: change to e.g. ⌃⌥K, Save → new combo works, old doesn't. Zero modifiers is rejected (previous kept).
- [ ] Accessibility not granted → status "Needs Accessibility permission", banner in Settings with button.

## Menu bar
- [ ] Loading: right after ⌥⌘G a "Đang kiểm tra…" spinner popover appears, then becomes the diff (or disappears on error).
- [ ] Status line: Ready / Checking… / Paused / Error / Needs Accessibility permission.
- [ ] Quit stops the helper (`pgrep grammar-helper` empty).

## Explanations
- [ ] Diff popover lists a short Vietnamese reason per fix under the text (setting `explain` in settings.json; set false for ~1.5s faster checks).

## Auto-check, quick fix, tone (manual)
- Tray → "Tự kiểm tra khi gõ" → "Hiện ngay". In Notes/TextEdit type `She dont like it.` and wait ~1s: the diff popover opens above the sentence's line; ⌘↩ / Áp dụng replaces only that sentence.
- Enable "Hotkey fixes at once": ⌥⌘G replaces directly; ⌘Z restores.
- ⌥⌘T on a selection → menu Formal/Friendly/Short → diff popover.
- Fix the same mistake twice → Settings → Insights lists it.
- Slack / Claude desktop / Chrome contenteditable: type `She dont like it.` → diff popover opens above the sentence (bounds come from the AXStaticText leaves). Click a toolbar button and back: no re-check.
- While a check runs: no spinner near the caret; the menu bar reads `ug…`. Popover hint shows only ⌘↩; Esc stays with the app.
- Keep typing right after the period → no popover yet; stop typing ~1.5s → it opens for the oldest checked sentence, one at a time.
- Type 3–4 wrong sentences quickly → each is checked in turn (queue of 3).
- Popover open, start the next sentence (type a letter) or leave it 8s → it closes; menu bar reads `ug 1`; tray → "Chờ sửa" lists it. Click it → diff + reasons → Áp dụng writes it back into its app (brought forward); Bỏ qua removes it.
- Delete a listed sentence from its field → it leaves the list. Unhandled entries drop after 10 min (max 10).
- Delete the sentence and type it again (also in another app): no new request (cached 10 min); a dismissed sentence never comes back this session. Changing endpoint/model/dictionary/explain/language re-checks.
- Engine down (wrong endpoint): no popover; the same sentence is retried only after 30s.
- One-line field (Chrome address bar, search box): type `xin chao ban hien.` → popover opens right under the field.
- Multi-line field without text bounds: popover opens at the focused window's top-right corner (else the screen's).
- Debug: `grep auto /tmp/proofy-debug.log` — `visible=false why=…` names the failing step (role, no value, no range).
