import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AppState, Notice } from "./types";

const BARS = 24;

const NOTICE_TEXT: Record<Notice["code"], string> = {
  device_lost: "Micro débranché",
  recording_failed: "Micro inaccessible",
  transcription_failed: "Échec de la transcription",
};

function formatTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function Overlay() {
  const [state, setState] = useState<AppState>({ kind: "idle" });
  const [notice, setNotice] = useState<Notice | null>(null);
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const [elapsed, setElapsed] = useState(0);
  const startedAt = useRef(0);

  useEffect(() => {
    const subs = [
      listen<AppState>("state", (e) => {
        setState(e.payload);
        if (e.payload.kind === "recording") {
          startedAt.current = performance.now();
          setElapsed(0);
          setLevels(Array(BARS).fill(0));
          setNotice(null);
        }
      }),
      listen<number>("level", (e) => setLevels((l) => [...l.slice(1), e.payload])),
      listen<Notice>("notice", (e) => setNotice(e.payload)),
    ];
    return () => {
      for (const s of subs) void s.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (state.kind !== "recording") return;
    const id = window.setInterval(() => setElapsed(performance.now() - startedAt.current), 250);
    return () => window.clearInterval(id);
  }, [state.kind]);

  if (notice && state.kind !== "recording" && state.kind !== "transcribing") {
    return (
      <div className="pill error" title={notice.detail}>
        <span className="dot" />
        <span className="label">{NOTICE_TEXT[notice.code]}</span>
      </div>
    );
  }

  if (state.kind === "transcribing") {
    return (
      <div className="pill">
        <span className="spinner" />
        <span className="label">Transcription…</span>
      </div>
    );
  }

  return (
    <div className="pill">
      <span className="dot recording" />
      <div className="wave" aria-hidden="true">
        {levels.map((l, i) => (
          <span key={i} style={{ transform: `scaleY(${Math.max(0.08, l)})` }} />
        ))}
      </div>
      <span className="time">{formatTime(elapsed)}</span>
    </div>
  );
}
