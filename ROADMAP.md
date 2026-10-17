# ugrammar UX roadmap (from brainstorm)

Status: Phases 1–4 implemented (autoCheck, quickFix, explainLang, ⌥⌘T tone menu, Insights). Chrome/Slack in-place replace and the UX of underlines still need manual verification (see TESTING.md). Auto-check stays off by default; AX polling is used, so no Input Monitoring permission.

Pain points: too many steps (select → hotkey → Accept); popover hides the text.
Direction: Grammarly-like auto-check, underline in place + small card. Both personal polish and distribution in parallel.

## Phase 1: auto-check on sentence end
- Trigger: sentence end (`.` `?` `!` or Enter). Check only the sentence just typed.
- Helper: watch typing (AX value/selection change or CGEventTap keyDown, needs Input Monitoring) and report the last sentence range + text.
- Engine: return per-error spans (original, replacement, reason) instead of whole-text diff; keep `<corrected>` fallback.
- Setting: `autoCheck` (default off), per-app exclusion already applies; skip secure fields.
- Cost guard: debounce, skip < 3 words, cache by sentence hash, cancel when user keeps typing.

## Phase 2: underline in place + small card
- Overlay (non-focusable, click-through panel) draws underline at `kAXBoundsForRangeParameterizedAttribute` per error.
- Click underline → small card next to the word: suggestion, short reason (selected language), Accept / Dismiss.
- Accept replaces only that range (AX set value on range; clipboard fallback).
- Limit: standard AX text fields only; Chromium needs `AXManualAccessibility` (already enabled). Elsewhere fall back to ⌥⌘G popover.
- Popover fix meanwhile: place beside the selected sentence, never over it.

## Phase 3: fewer steps for manual flow
- Accept-without-popover shortcut (fix + replace at once, toast with Undo ⌘Z).
- Auto-accept when the change is tiny (optional setting).

## Phase 4: more features
- Explanation language: setting (Vietnamese default); prompt takes the language.
- Tone: second hotkey ⌥⌘T → menu Formal / Friendly / Short for the selection.
- Learn from repeated mistakes: store counts locally; Settings → Insights shows top repeated errors with a short note.

## Distribution (parallel)
- Rebuild + release v0.1.1 (showBadge off by default).
- Onboarding for Accessibility (+ Input Monitoring if CGEventTap).
- Notarization + auto-update (needs Apple Developer ID).
- Site: 1200×630 og image, refresh screenshots after Phase 2.

## Open questions
- Input Monitoring permission acceptable, or stay with AX notifications only?
- Token cost of auto-check on a paid model vs local model default.
