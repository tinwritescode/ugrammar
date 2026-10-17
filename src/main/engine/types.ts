export interface CheckRequest {
  text: string;
  dictionary: string[];
  /** Ask for a short Vietnamese reason per fix. Slower; off by default in the engine. */
  explain?: boolean;
  /** Language for the per-fix reasons. Default Vietnamese. */
  explainLang?: string;
  /** Rewrite in this tone (also fixes grammar). */
  tone?: Tone;
}

export type Tone = "formal" | "friendly" | "short";

export interface CheckResult {
  original: string;
  corrected: string;
  notes?: string[];
}

export type EngineErrorKind = "timeout" | "network" | "http" | "bad_response";

export class EngineError extends Error {
  constructor(public kind: EngineErrorKind, message: string) {
    super(message);
    this.name = "EngineError";
  }
}

/** Pluggable grammar engine. Implementations must never mutate the input; errors throw EngineError. */
export interface GrammarEngine {
  check(req: CheckRequest): Promise<CheckResult>;
  testConnection(): Promise<{ ok: boolean; message: string }>;
}
