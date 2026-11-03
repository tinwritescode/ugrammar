import { proofy } from "./bridge";
import { el } from "./dom";
import { diffNodes, fixCount, note } from "./fix";
import { dict } from "./i18n";
import { mascot } from "./mascot";
import type { PopoverPayload } from "../shared/ipc";

const card = document.getElementById("card")!;

function render(p: PopoverPayload) {
  const t = dict(p.lang);
  document.documentElement.lang = p.lang;
  card.replaceChildren();
  const compact = p.kind !== "diff";
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
  } else {
    const ops = p.ops ?? [];
    const count = fixCount(ops);
    const meta = el("div", { className: "meta" }, mascot("pen", 50),
      el("div", { className: "who" }, el("strong", {}, t.owlFixed(count)), ...(p.appName ? [el("span", {}, t.inApp(p.appName))] : [])));
    const body = el("div", { className: "text" }, ...diffNodes(ops));
    card.append(meta, body);
    if (p.notes?.length) card.append(el("ul", { className: "notes" }, ...p.notes.map(note)));
    const accept = el("button", { className: "primary", onclick: () => proofy.popoverAction("accept") }, t.apply);
    const dismiss = el("button", { onclick: () => proofy.popoverAction("dismiss") }, t.dismiss);
    const hint = el("span", { className: "hint" }, el("kbd", {}, "⌘↩"), el("span", {}, t.hintApply));
    if (p.esc) hint.append(el("kbd", {}, "esc"), el("span", {}, t.hintDismiss));
    card.append(el("div", { className: "bar" }, hint, dismiss, accept));
  }
  requestAnimationFrame(() => proofy.popoverResize(card.offsetHeight, compact ? card.offsetWidth : undefined));
}

proofy.onPopover(render);
