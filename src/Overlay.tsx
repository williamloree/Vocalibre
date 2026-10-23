import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type AppState, type Notice } from "./api";

const BARS = 24;
/** A drag is over once the window stops moving for this long. */
const DRAG_SETTLE_MS = 400;

function formatTime(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function Overlay() {
  const { t } = useTranslation();
  const [state, setState] = useState<AppState>({ kind: "idle" });
  const [notice, setNotice] = useState<Notice | null>(null);
  const [levels, setLevels] = useState<number[]>(() => Array<number>(BARS).fill(0));
  const [elapsed, setElapsed] = useState(0);
  const startedAt = useRef(0);
  const dragging = useRef(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let settle: number | undefined;
    const subs = [
      listen<AppState>("state", (e) => {
        setState(e.payload);
        if (e.payload.kind === "recording") {
          startedAt.current = performance.now();
          setElapsed(0);
          setLevels(Array<number>(BARS).fill(0));
          setNotice(null);
        }
      }),
      listen<number>("level", (e) => setLevels((l) => [...l.slice(1), e.payload])),
      listen<Notice>("notice", (e) => setNotice(e.payload)),
      // Only moves made by the user are saved, not our own placement.
      win.onMoved(() => {
        if (!dragging.current) return;
        window.clearTimeout(settle);
        settle = window.setTimeout(() => {
          dragging.current = false;
          void api.overlayMoved();
        }, DRAG_SETTLE_MS);
      }),
    ];
    return () => {
      window.clearTimeout(settle);
      for (const s of subs) void s.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (state.kind !== "recording") return;
    const id = window.setInterval(() => setElapsed(performance.now() - startedAt.current), 250);
    return () => window.clearInterval(id);
  }, [state.kind]);

  const startDrag = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    dragging.current = true;
    void getCurrentWindow().startDragging();
  };

  let content;
  if (notice && state.kind !== "recording" && state.kind !== "transcribing") {
    content = (
      <>
        <span className="dot warn" />
        <span className="label" title={notice.detail}>
          {t(`overlay.notice.${notice.code}`)}
        </span>
      </>
    );
  } else if (state.kind === "transcribing") {
    content = (
      <>
        <span className="spinner" />
        <span className="label">{t("overlay.transcribing")}</span>
      </>
    );
  } else {
    content = (
      <>
        <span className="dot recording" />
        <div className="wave" aria-hidden="true">
          {levels.map((l, i) => (
            <span key={i} style={{ transform: `scaleY(${Math.max(0.08, l)})` }} />
          ))}
        </div>
        <span className="time">{formatTime(elapsed)}</span>
      </>
    );
  }

  return (
    <div className="pill" onPointerDown={startDrag}>
      {content}
    </div>
  );
}
