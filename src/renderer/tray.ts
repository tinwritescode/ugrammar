import type { ProofyApi, TrayState } from "../shared/ipc";
import type { Settings } from "../main/settings";
declare const proofy: ProofyApi;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const panel = $("panel");

const STATUS_TEXT: Record<TrayState["status"], string> = {
  idle: "Sẵn sàng", checking: "Đang kiểm tra…", paused: "Đã tạm dừng", error: "Có lỗi", needs_permission: "Cần cấp quyền",
};

let state: TrayState | null = null;

const save = (patch: Partial<Settings>) => void proofy.saveSettings(patch);

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, ...kids: (Node | string)[]) {
  const e = document.createElement(tag);
  e.className = cls;
  e.append(...kids);
  return e;
}

function host(endpoint: string) {
  try { return new URL(endpoint).host; } catch { return endpoint; }
}

function render(s: TrayState) {
  state = s;
  const cfg = s.settings;
  panel.dataset.status = s.status;
  panel.dataset.paused = String(cfg.paused);
  $("statusText").textContent = s.message && s.status === "error" ? `${STATUS_TEXT.error} — ${s.message}` : STATUS_TEXT[s.status];
  $("heroTitle").textContent = cfg.paused ? "Đang nghỉ" : "Đang canh chính tả";
  $("hotkey").textContent = s.hotkey;
  $("perm").hidden = s.status !== "needs_permission";
  const mode = cfg.autoCheck ? cfg.autoCheckMode : "off";
  for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) b.classList.toggle("on", b.dataset.mode === mode);
  for (const c of document.querySelectorAll<HTMLInputElement>(".switch")) c.checked = cfg[c.dataset.key as "quickFix" | "explain" | "showBadge"];
  $("insights").replaceChildren(...(s.insights.length
    ? s.insights.map((i) => el("li", "", el("span", "pair", el("del", "", i.from || "∅"), " → ", el("ins", "", i.to || "∅")), el("span", "count", `${i.count}×`)))
    : [el("li", "empty", "Chưa có lỗi lặp lại. Tốt lắm!")]));
  $("engine").textContent = `${cfg.model} · ${host(cfg.endpoint)}`;
  $("engine").title = cfg.endpoint;
  requestAnimationFrame(() => proofy.trayResize(panel.offsetHeight));
}

$("power").onclick = () => state && save({ paused: !state.settings.paused });
$("openSettings").onclick = () => proofy.trayAction("settings");
$("openAX").onclick = () => proofy.trayAction("ax");
$("quit").onclick = () => proofy.trayAction("quit");
for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) {
  b.onclick = () => save(b.dataset.mode === "off" ? { autoCheck: false } : { autoCheck: true, autoCheckMode: b.dataset.mode === "popup" ? "popup" : "underline" });
}
for (const b of document.querySelectorAll<HTMLButtonElement>("[data-tone]")) {
  b.onclick = () => proofy.trayAction(b.dataset.tone === "friendly" ? "friendly" : b.dataset.tone === "short" ? "short" : "formal");
}
for (const c of document.querySelectorAll<HTMLInputElement>(".switch")) {
  c.onchange = () => save({ [c.dataset.key as "quickFix" | "explain" | "showBadge"]: c.checked });
}

proofy.onTray(render);
// Replay the drop-in each time the panel opens, not on live updates.
window.addEventListener("focus", () => { panel.style.animation = "none"; void panel.offsetWidth; panel.style.animation = ""; });
proofy.getTray().then(render);
