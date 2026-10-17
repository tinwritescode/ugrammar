import type { DiffOp } from "./diff";
import type { Settings } from "../main/settings";

export interface PopoverPayload {
  kind: "diff" | "message" | "loading";
  original?: string;
  corrected?: string;
  ops?: DiffOp[];
  message?: string;
  appName?: string;
}

export interface SettingsView {
  settings: Settings;
  hasApiKey: boolean;
  axTrusted: boolean;
}

export interface TestResult {
  ok: boolean;
  message: string;
}

export interface ProofyApi {
  onPopover(cb: (p: PopoverPayload) => void): void;
  popoverAction(action: "accept" | "dismiss"): void;
  popoverResize(height: number): void;
  getSettings(): Promise<SettingsView>;
  saveSettings(patch: Partial<Settings>): Promise<SettingsView>;
  setApiKey(key: string): Promise<SettingsView>;
  testConnection(draft: { endpoint: string; model: string; apiKey?: string }): Promise<TestResult>;
  pickApp(): Promise<string | null>;
  openAccessibilitySettings(): Promise<void>;
}
