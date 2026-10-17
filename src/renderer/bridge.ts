import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import type { PopoverPayload, ProofyApi, TrayState } from "../shared/ipc";

export const proofy: ProofyApi = {
  onPopover: (cb) => {
    void listen<PopoverPayload>("popover:show", (e) => cb(e.payload));
    // The first show can land before this listener is registered.
    void invoke<PopoverPayload | null>("popover_current").then((p) => p && cb(p));
  },
  popoverAction: (action) => void invoke("popover_action", { action }),
  popoverResize: (height, width) => void invoke("popover_resize", { height, width }),
  badgeClick: () => void invoke("badge_click"),
  badgeMenu: () => void invoke("badge_menu"),
  underlineClick: () => void invoke("underline_click"),
  clearInsights: () => invoke("clear_insights"),
  saveSettings: (patch) => invoke("save_settings", { patch }),
  setApiKey: (key) => invoke("set_api_key", { key }),
  testConnection: (draft) => invoke("test_connection", { draft }),
  pickApp: () => invoke("pick_app"),
  getTray: () => invoke("get_tray"),
  onTray: (cb) => void listen<TrayState>("tray:state", (e) => cb(e.payload)),
  trayAction: (action) => void invoke("tray_action", { action }),
  trayResize: (height) => void invoke("tray_resize", { height }),
  appVersion: getVersion,
};
