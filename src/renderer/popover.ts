import { proofy } from "./bridge";
import { el } from "./dom";
import { dictionaryWord, fixCount, fixToggles, note } from "./fix";
import { dict } from "./i18n";
import { mascot } from "./mascot";
import type { PopoverPayload } from "../shared/ipc";

const card = document.getElementById("card")!;

function render(p: PopoverPayload) {
  const t = dict(p.lang);
  document.documentElement.lang = p.lang;
  card.replaceChildren();
  const compact = p.kind === "message" || p.kind === "loading";
  card.className = p.kind === "loading" ? "pill" : p.kind === "message" ? "compact" : "";
  void card.offsetWidth; // restart the pop-in animation
  card.classList.add("pop");
  if (p.kind === "loading") {
    card.append(mascot("checking", 34), p.message ?? t.checking);
  } else if (p.kind === "message") {
    // A trailing ✓ marks a success message.
    const text = p.message ?? "";
    const ok = text.endsWith("✓");
    card.append(el("div", { className: "msg" }, ...(ok ? [mascot("happy", 38), text.slice(0, -1).trim()] : [mascot("confused", 38), text])));
  } else if (p.kind === "note") {
    const meta = el("div", { className: "meta" }, mascot("reading", 50), el("div", { className: "who" }, el("strong", {}, t.howItReads)));
    const close = el("button", { className: "primary", onclick: () => proofy.popoverAction("dismiss") }, t.close);
    const hint = el("span", { className: "hint" }, el("kbd", {}, "esc"), el("span", {}, t.close.toLowerCase()));
    card.append(meta, el("div", { className: "text" }, p.message ?? ""), el("div", { className: "bar" }, hint, close));
  } else {
    const ops = p.ops ?? [];
    const count = fixCount(ops);
    const meta = el("div", { className: "meta" }, mascot("pen", 50),
      el("div", { className: "who" }, el("strong", {}, t.owlFixed(count)), ...(p.appName ? [el("span", {}, t.inApp(p.appName))] : [])));
    const accept = el("button", { className: "primary", onclick: () => proofy.popoverAction("accept") }, t.apply);
    // Skipped one-word fixes offer "Always keep": the word goes into the dictionary.
    const keep = el("div", { className: "keep" });
    const added = new Set<string>();
    const toggled = (skipped: number[]) => {
      accept.disabled = skipped.length === count;
      proofy.popoverSkip(skipped);
      const words = [...new Set(skipped.map((n) => dictionaryWord(originals[n])).filter((w) => w !== null))];
      keep.replaceChildren(...(words.length ? [el("span", {}, t.alwaysKeep)] : []), ...words.map((w) => {
        const b = el("button", { disabled: added.has(w) }, added.has(w) ? `✓ ${w}` : `+ ${w}`);
        b.onclick = () => {
          added.add(w);
          proofy.addWord(w);
          b.disabled = true;
          b.textContent = `✓ ${w}`;
        };
        return b;
      }));
      requestAnimationFrame(() => proofy.popoverResize(card.offsetHeight));
    };
    const { nodes, originals } = fixToggles(ops, { skip: t.skipFix, keep: t.keepFix }, toggled);
    const body = el("div", { className: "text" }, ...nodes);
    card.append(meta, body, keep);
    if (p.notes?.length) card.append(el("ul", { className: "notes" }, ...p.notes.map((n) => note(n, t.fixKinds))));
    const dismiss = el("button", { onclick: () => proofy.popoverAction("dismiss") }, t.dismiss);
    const hint = el("span", { className: "hint" }, el("kbd", {}, "⌘↩"), el("span", {}, t.hintApply));
    if (p.esc) hint.append(el("kbd", {}, "esc"), el("span", {}, t.hintDismiss));
    card.append(el("div", { className: "bar" }, hint, dismiss, accept));
  }
  requestAnimationFrame(() => proofy.popoverResize(card.offsetHeight, compact ? card.offsetWidth : undefined));
}

proofy.onPopover(render);
