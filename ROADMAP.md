# ugrammar UX roadmap (from brainstorm)

Status: Phases 1–4 implemented (autoCheck, quickFix, explainLang, ⌥⌘T tone menu, Insights). Chrome/Slack in-place replace still needs manual verification (see TESTING.md). Auto-check stays off by default; AX polling is used, so no Input Monitoring permission.

Pain points: too many steps (select → hotkey → Accept); popover hides the text.
Direction: Grammarly-like auto-check, popup with the fix + reasons. Both personal polish and distribution in parallel.

## Phase 1: auto-check on sentence end
- Trigger: sentence end (`.` `?` `!` or Enter). Check only the sentence just typed.
- Helper: watch typing (AX value/selection change or CGEventTap keyDown, needs Input Monitoring) and report the last sentence range + text.
- Engine: return per-error spans (original, replacement, reason) instead of whole-text diff; keep `<corrected>` fallback.
- Setting: `autoCheck` (default off), per-app exclusion already applies; skip secure fields.
- Popup opens the sentence diff + reasons right away, only if the user has not typed past it.
- Cost guard: debounce, skip < 3 words, cache by sentence hash, cancel when user keeps typing.

## Phase 2: underline in place + small card (dropped)
- Dropped: underlines and the typing badge were removed as not needed in real use. Auto-check uses the sentence popup only.
- Popover fix meanwhile: place beside the selected sentence, never over it.

## Phase 3: fewer steps for manual flow
- Accept-without-popover shortcut (fix + replace at once, toast with Undo ⌘Z).
- Auto-accept when the change is tiny (optional setting).

## Phase 4: more features
- Explanation language: setting (Vietnamese default); prompt takes the language.
- Tone: second hotkey ⌥⌘T → menu Formal / Friendly / Short for the selection.
- Learn from repeated mistakes: store counts locally; Settings → Insights shows top repeated errors with a short note.

## Distribution (parallel)
- Rebuild + release v0.1.1 (showBadge off by default). - Release v0.1.2: Slack/contenteditable underlines, popup mode, loading pill.
- Onboarding for Accessibility (+ Input Monitoring if CGEventTap).
- Notarization + auto-update (needs Apple Developer ID).
- Site: 1200×630 og image, refresh screenshots after Phase 2.

## Open questions
- Input Monitoring permission acceptable, or stay with AX notifications only?
- Token cost of auto-check on a paid model vs local model default.
