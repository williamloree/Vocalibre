// Typed wrappers around the Rust commands and the shapes they exchange.
import { invoke } from "@tauri-apps/api/core";

export type Lang = "fr" | "en";

export type AppState =
  | { kind: "onboarding" }
  | { kind: "loading_model"; model_id: string }
  | { kind: "idle" }
  | { kind: "recording"; started_at_ms: number; mode: RecordingMode }
  | { kind: "transcribing"; job_id: number }
  | { kind: "error"; error: { code: string; detail: string; retry_model: string | null } };

export type Notice = {
  code: "device_lost" | "recording_failed" | "transcription_failed";
  detail: string;
};

export type RecordingMode = "toggle" | "push_to_talk";

export type Replacement = { from: string; to: string };

export type Settings = {
  schema_version: number;
  ui_language: "system" | "fr" | "en";
  autostart: boolean;
  sounds_enabled: boolean;
  sound_volume: number;
  overlay_enabled: boolean;
  history_enabled: boolean;
  onboarding_done: boolean;
  active_model: string | null;
  models_dir: string | null;
  language: string;
  vocabulary: string;
  replacements: Replacement[];
  capitalize_first: boolean;
  strip_final_period: boolean;
  auto_space: boolean;
  beam_size: number;
  microphone: string | null;
  acceleration: "auto" | "cpu" | "gpu";
  threads: number;
  max_duration_s: number;
  silence_auto_stop: boolean;
  silence_timeout_s: number;
  /** `null` = disabled; at least one of the two dictation shortcuts is set. */
  hotkey: string | null;
  ptt_hotkey: string | null;
  paste_last_hotkey: string | null;
  cancel_key: string;
  injection_mode: "paste" | "type";
  restore_delay_ms: number;
  keep_in_clipboard: boolean;
  check_updates: boolean;
  debug_log_text: boolean;
  overlay_positions: Record<string, { x: number; y: number }>;
};

export type Status = {
  state: AppState;
  lang: Lang;
  model: { id: string; on_gpu: boolean; fell_back_to_cpu: boolean } | null;
  hotkey_error: string | null;
  models_dir: string;
  vad_file: string;
};

export type ModelEntry = {
  id: string;
  display_name: string;
  file: { url: string; file_name: string; sha256: string; size: number };
  languages: string[];
  ram_mb: number;
  vram_mb: number;
  speed: number;
  accuracy: number;
  supports_prompt: boolean;
  recommended_for: ("gpu" | "cpu_strong" | "cpu_weak")[];
  installed: boolean;
  active: boolean;
};

export type BenchResult = { audio_ms: number; inference_ms: number; text: string };

export type MicInfo = { id: string; name: string; is_default: boolean };

export type HistoryEntry = {
  id: number;
  text: string;
  at: string;
  duration_ms: number;
  model_id: string;
};

export type AppInfo = { version: string; config_dir: string; log_dir: string };

export const api = {
  status: () => invoke<Status>("get_status"),
  lang: () => invoke<Lang>("get_lang"),
  settings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),
  resetSettings: () => invoke<Settings>("reset_settings"),
  suspendHotkey: (suspended: boolean) => invoke<void>("suspend_hotkey", { suspended }),
  models: () => invoke<ModelEntry[]>("list_models"),
  selectModel: (id: string) => invoke<void>("select_model", { id }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  benchmark: () => invoke<BenchResult>("benchmark"),
  microphones: () => invoke<MicInfo[]>("list_microphones"),
  history: () => invoke<HistoryEntry[]>("get_history"),
  clearHistory: () => invoke<void>("clear_history"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  openLogsDir: () => invoke<void>("open_logs_dir"),
  openModelsDir: () => invoke<void>("open_models_dir"),
  overlayMoved: () => invoke<void>("overlay_moved"),
  appInfo: () => invoke<AppInfo>("app_info"),
};
