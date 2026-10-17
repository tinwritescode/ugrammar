import type { DiffOp } from "./diff";
import type { Settings } from "../main/settings";
import type { Tone } from "../main/engine/types";
import type { StatusKind } from "../main/flow";

export interface PopoverPayload {
  kind: "diff" | "message" | "loading";
  original?: string;
  corrected?: string;
  ops?: DiffOp[];
  message?: string;
  appName?: string;
  notes?: string[];
}

export interface SettingsView {
  settings: Settings;
  hasApiKey: boolean;
  axTrusted: boolean;
  insights: { from: string; to: string; count: number; note?: string }[];
}

export interface TrayState {
  status: StatusKind;
  message?: string;
  settings: Settings;
  hotkey: string;
  insights: SettingsView["insights"];
}

export type TrayAction = "settings" | "quit" | "ax" | Tone;

export interface TestResult {
  ok: boolean;
  message: string;
}

export interface ProofyApi {
  onPopover(cb: (p: PopoverPayload) => void): void;
  popoverAction(action: "accept" | "dismiss"): void;
  popoverResize(height: number, width?: number): void;
  badgeClick(): void;
  badgeMenu(): void;
  underlineClick(): void;
  clearInsights(): Promise<SettingsView>;
  getSettings(): Promise<SettingsView>;
  saveSettings(patch: Partial<Settings>): Promise<SettingsView>;
  setApiKey(key: string): Promise<SettingsView>;
  testConnection(draft: { endpoint: string; model: string; apiKey?: string }): Promise<TestResult>;
  pickApp(): Promise<string | null>;
  openAccessibilitySettings(): Promise<void>;
  getTray(): Promise<TrayState>;
  onTray(cb: (s: TrayState) => void): void;
  trayAction(a: TrayAction): void;
  trayResize(height: number): void;
}
