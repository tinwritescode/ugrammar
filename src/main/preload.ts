import { contextBridge, ipcRenderer } from "electron";
import type { ProofyApi } from "../shared/ipc";

const api: ProofyApi = {
  onPopover: (cb) => ipcRenderer.on("popover:show", (_e, p) => cb(p)),
  popoverAction: (a) => ipcRenderer.send("popover:action", a),
  badgeMenu: () => ipcRenderer.send("badge:menu"),
  underlineClick: () => ipcRenderer.send("underline:click"),
  clearInsights: () => ipcRenderer.invoke("settings:clearInsights"),
  badgeClick: () => ipcRenderer.send("badge:click"),
  popoverResize: (h) => ipcRenderer.send("popover:resize", h),
  getSettings: () => ipcRenderer.invoke("settings:get"),
  saveSettings: (p) => ipcRenderer.invoke("settings:save", p),
  setApiKey: (k) => ipcRenderer.invoke("settings:setKey", k),
  testConnection: (d) => ipcRenderer.invoke("settings:test", d),
  pickApp: () => ipcRenderer.invoke("settings:pickApp"),
  openAccessibilitySettings: () => ipcRenderer.invoke("settings:openAX"),
};
contextBridge.exposeInMainWorld("proofy", api);
