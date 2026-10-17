export type DiffOp = { type: "equal" | "delete" | "insert"; text: string };
export type Tone = "formal" | "friendly" | "short";
export type StatusKind = "idle" | "checking" | "paused" | "error" | "needs_permission";

export interface Hotkey {
  key: string;
  mods: ("cmd" | "alt" | "ctrl" | "shift")[];
}

export interface Settings {
  endpoint: string;
  model: string;
  timeoutMs: number;
  dictionary: string[];
  excludedApps: string[];
  hotkey: Hotkey;
  paused: boolean;
  explain: boolean;
  showBadge: boolean;
  autoCheck: boolean;
  autoCheckMode: "underline" | "popup";
  quickFix: boolean;
  explainLang: string;
  firstRunAcknowledged: boolean;
}

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
  hasApiKey: boolean;
}

export type TrayAction = "quit" | "ax" | "github" | Tone;

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
  saveSettings(patch: Partial<Settings>): Promise<SettingsView>;
  setApiKey(key: string): Promise<SettingsView>;
  testConnection(draft: { endpoint: string; model: string; apiKey?: string }): Promise<TestResult>;
  pickApp(): Promise<string | null>;
  openAccessibilitySettings(): Promise<void>;
  getTray(): Promise<TrayState>;
  onTray(cb: (s: TrayState) => void): void;
  trayAction(a: TrayAction): void;
  trayResize(height: number): void;
  appVersion(): Promise<string>;
}
