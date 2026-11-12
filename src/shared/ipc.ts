// Types the Rust side sends or reads are generated from it into ./bindings (src-tauri's
// `typescript_bindings_are_current` test rewrites them); only UI-side types are written here.
import type { ConnectionStatus } from "./bindings/ConnectionStatus";
import type { PopoverPayload } from "./bindings/PopoverPayload";
import type { Settings } from "./bindings/Settings";
import type { AppInfo } from "./bindings/AppInfo";
import type { TrayState } from "./bindings/TrayState";
import type { Tone } from "./bindings/Tone";
import type { PermissionStatus } from "./bindings/PermissionStatus";

export type { DiffOp } from "./bindings/DiffOp";
export type { Hotkey } from "./bindings/Hotkey";
export type { Insight } from "./bindings/Insight";
export type { Lang } from "./bindings/Lang";
export type { PendingFix } from "./bindings/PendingFix";
export type { PermissionId } from "./bindings/PermissionId";
export type { Provider } from "./bindings/Provider";
export type { StatusKind } from "./bindings/StatusKind";
export type { UpdateNote } from "./bindings/UpdateNote";
export type { AppInfo, PermissionStatus, PopoverPayload, Settings, Tone, TrayState };
export type TestResult = ConnectionStatus;

/** Engine settings typed in the setup view, tested before they are saved. */
export type EngineDraft = Pick<Settings, "provider" | "endpoint" | "model" | "claudeModel" | "claudePath">;

export type TrayAction = "quit" | "ax" | "reset_ax" | "reveal_app" | "github" | "license" | "restart" | Tone;

export interface ProofyApi {
  onPopover(cb: (p: PopoverPayload) => void): void;
  popoverAction(action: "accept" | "dismiss"): void;
  pendingAction(id: number, action: "accept" | "dismiss"): void;
  popoverResize(height: number, width?: number): void;
  checkUpdate(): void;
  clearInsights(): Promise<void>;
  saveSettings(patch: Partial<Settings>): Promise<void>;
  setLaunchAtLogin(enabled: boolean): Promise<void>;
  setApiKey(key: string): Promise<void>;
  testConnection(draft: { settings: EngineDraft; apiKey?: string }): Promise<TestResult>;
  pickApp(): Promise<string | null>;
  appInfo(ids: string[]): Promise<AppInfo[]>;
  getTray(): Promise<TrayState>;
  onTray(cb: (s: TrayState) => void): void;
  trayAction(a: TrayAction): void;
  trayResize(height: number): void;
  appVersion(): Promise<string>;
  /** Re-probes every permission; the permissions screen polls this while it's open. */
  checkPermissions(): Promise<PermissionStatus[]>;
  /** Keeps the panel up when it loses focus, while the user grants a permission in System Settings. */
  pinTray(pinned: boolean): void;
  /** Starts a native drag of the app itself, to drop on the Accessibility list. */
  dragApp(): void;
  /** Fired when launch itself opens the panel on the permissions screen (missing/stale Accessibility). */
  onOpenPermissions(cb: () => void): void;
}
