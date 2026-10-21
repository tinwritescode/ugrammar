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
  autoCheck: boolean;
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
  /** Esc dismisses it. */
  esc: boolean;
}

export interface Insight {
  from: string;
  to: string;
  count: number;
  note?: string;
}

export interface TrayState {
  status: StatusKind;
  message?: string;
  settings: Settings;
  hotkey: string;
  insights: Insight[];
  hasApiKey: boolean;
  update?: string;
  updateNote?: UpdateNote;
  /** Auto-check fixes not acted on yet, oldest first. */
  pending: PendingFix[];
}

export interface PendingFix {
  id: number;
  appName?: string;
  ops: DiffOp[];
  notes: string[];
}

export type UpdateNote = "checking" | "downloading" | "up_to_date" | "failed" | "unsupported";

export type TrayAction = "quit" | "ax" | "github" | "license" | "restart" | Tone;

/** An excluded app; `name`/`icon` are absent when it is not installed. `icon` is a base64 PNG. */
export interface AppInfo {
  id: string;
  name?: string;
  icon?: string;
}

export interface TestResult {
  ok: boolean;
  message: string;
}

export interface ProofyApi {
  onPopover(cb: (p: PopoverPayload) => void): void;
  popoverAction(action: "accept" | "dismiss"): void;
  pendingAction(id: number, action: "accept" | "dismiss"): void;
  popoverResize(height: number, width?: number): void;
  checkUpdate(): void;
  clearInsights(): Promise<void>;
  saveSettings(patch: Partial<Settings>): Promise<void>;
  setApiKey(key: string): Promise<void>;
  testConnection(draft: { endpoint: string; model: string; apiKey?: string }): Promise<TestResult>;
  pickApp(): Promise<string | null>;
  appInfo(ids: string[]): Promise<AppInfo[]>;
  getTray(): Promise<TrayState>;
  onTray(cb: (s: TrayState) => void): void;
  trayAction(a: TrayAction): void;
  trayResize(height: number): void;
  appVersion(): Promise<string>;
}
