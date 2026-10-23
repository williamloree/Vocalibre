// Mirrors `AppState` and the notice payload emitted by the Rust shell.

export type AppState =
  | { kind: "onboarding" }
  | { kind: "loading_model"; model_id: string }
  | { kind: "idle" }
  | { kind: "recording"; started_at_ms: number; mode: "toggle" | "push_to_talk" }
  | { kind: "transcribing"; job_id: number }
  | { kind: "error"; error: { code: string; detail: string; retry_model: string | null } };

export type Notice = {
  code: "device_lost" | "recording_failed" | "transcription_failed";
  detail: string;
};
