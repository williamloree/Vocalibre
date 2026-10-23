import { useEffect, useState, type ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { StoreProvider, useStore } from "./store";
import { General } from "./pages/General";
import { Models } from "./pages/Models";
import { Transcription } from "./pages/Transcription";
import { Hardware } from "./pages/Hardware";
import { Shortcuts } from "./pages/Shortcuts";
import { Injection } from "./pages/Injection";
import { History } from "./pages/History";
import { Advanced } from "./pages/Advanced";
import { About } from "./pages/About";

const PAGES = {
  general: General,
  models: Models,
  transcription: Transcription,
  hardware: Hardware,
  shortcuts: Shortcuts,
  injection: Injection,
  history: History,
  advanced: Advanced,
  about: About,
} satisfies Record<string, ComponentType>;

type Page = keyof typeof PAGES;

function isPage(p: string): p is Page {
  return p in PAGES;
}

function pageFromHash(): Page {
  const p = window.location.hash.replace(/^#\/?/, "");
  return isPage(p) ? p : "general";
}

export function SettingsApp() {
  return (
    <StoreProvider>
      <Layout />
    </StoreProvider>
  );
}

function Layout() {
  const { t } = useTranslation();
  const { status, error } = useStore();
  const [page, setPage] = useState<Page>(pageFromHash);

  useEffect(() => {
    const unlisten = listen<string>("navigate", (e) => {
      if (isPage(e.payload)) setPage(e.payload);
    });
    return () => void unlisten.then((u) => u());
  }, []);

  useEffect(() => {
    window.history.replaceState(null, "", `#/${page}`);
  }, [page]);

  const Current = PAGES[page];
  return (
    <div className="layout">
      <nav className="sidebar" aria-label="Vocalibre">
        <div className="brand">
          <img src="/logo.png" alt="" width={28} height={28} />
          <span>Vocalibre</span>
        </div>
        {(Object.keys(PAGES) as Page[]).map((p) => (
          <button
            key={p}
            type="button"
            className={p === page ? "nav-item active" : "nav-item"}
            aria-current={p === page ? "page" : undefined}
            onClick={() => setPage(p)}
          >
            {t(`nav.${p}`)}
          </button>
        ))}
        {status && (
          <div className={`state state-${status.state.kind}`}>
            <span className="state-dot" />
            {t(`state.${status.state.kind}`)}
          </div>
        )}
      </nav>
      <main className="content">
        <h1>{t(`nav.${page}`)}</h1>
        <Current />
      </main>
      {error && (
        <div className="toast" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
