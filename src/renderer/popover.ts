import type { ProofyApi, PopoverPayload } from "../shared/ipc";
declare const proofy: ProofyApi;

const card = document.getElementById("card")!;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, props: Partial<HTMLElementTagNameMap[K]> = {}, ...kids: (Node | string)[]) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

function render(p: PopoverPayload) {
  card.replaceChildren();
  if (p.kind === "loading") {
    card.append(el("div", { className: "loading" }, el("span", { className: "spin" }), p.message ?? "Checking…"));
  } else if (p.kind === "message") {
    card.append(el("div", { className: "msg" }, p.message ?? ""));
  } else {
    const body = el("div", { className: "text" });
    for (const op of p.ops ?? []) {
      if (op.type === "equal") body.append(op.text);
      else body.append(el(op.type === "insert" ? "ins" : "del", {}, op.text));
    }
    const accept = el("button", { className: "primary", onclick: () => proofy.popoverAction("accept") }, "Accept");
    const dismiss = el("button", { onclick: () => proofy.popoverAction("dismiss") }, "Dismiss");
    card.append(body, el("div", { className: "bar" }, el("span", { className: "hint" }, "⌘↩ accept · esc dismiss"), dismiss, accept));
  }
  requestAnimationFrame(() => proofy.popoverResize(card.offsetHeight + 14));
}

proofy.onPopover(render);
