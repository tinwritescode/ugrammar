import { proofy } from "./bridge";
import { el } from "./dom";
import { diffNodes, fixCount, note } from "./fix";
import type { PopoverPayload } from "../shared/ipc";

const card = document.getElementById("card")!;

const checkIcon = () => {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.innerHTML = '<path d="M20 6 9 17l-5-5"/>';
  return el("span", { className: "check", ariaHidden: "true" }, s);
};

function render(p: PopoverPayload) {
  card.replaceChildren();
  const compact = p.kind !== "diff";
  card.className = p.kind === "loading" ? "pill" : p.kind === "message" ? "compact" : "";
  void card.offsetWidth; // restart the pop-in animation
  card.classList.add("pop");
  if (p.kind === "loading") {
    card.append(el("span", { className: "spin" }), p.message ?? "Đang kiểm tra…");
  } else if (p.kind === "message") {
    // A trailing ✓ marks a success message.
    const text = p.message ?? "";
    const ok = text.endsWith("✓");
    card.append(el("div", { className: "msg" }, ...(ok ? [checkIcon(), text.slice(0, -1).trim()] : [text])));
  } else {
    const ops = p.ops ?? [];
    const count = fixCount(ops);
    const meta = el("div", { className: "meta" });
    if (p.appName) meta.append(el("strong", {}, p.appName), el("span", {}, "·"));
    meta.append(el("span", {}, `${count} lỗi`));
    const body = el("div", { className: "text" }, ...diffNodes(ops));
    card.append(meta, body);
    if (p.notes?.length) card.append(el("ul", { className: "notes" }, ...p.notes.map(note)));
    const accept = el("button", { className: "primary", onclick: () => proofy.popoverAction("accept") }, "Áp dụng");
    const dismiss = el("button", { onclick: () => proofy.popoverAction("dismiss") }, "Bỏ qua");
    const hint = el("span", { className: "hint" }, el("kbd", {}, "⌘↩"), el("span", {}, "áp dụng"));
    if (p.esc) hint.append(el("kbd", {}, "esc"), el("span", {}, "bỏ qua"));
    card.append(el("div", { className: "bar" }, hint, dismiss, accept));
  }
  requestAnimationFrame(() => proofy.popoverResize(card.offsetHeight, compact ? card.offsetWidth : undefined));
}

proofy.onPopover(render);
