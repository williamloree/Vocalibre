// Settings shared by every page: changes are sent to the backend at once,
// which validates, applies and saves them.
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { api, type Settings, type Status } from "../api";

type Store = {
  settings: Settings;
  status: Status | null;
  update: (patch: Partial<Settings>) => Promise<boolean>;
  replace: (settings: Settings) => void;
  error: string | null;
  showError: (raw: unknown) => void;
};

const StoreContext = createContext<Store | null>(null);

export function useStore(): Store {
  const store = useContext(StoreContext);
  if (!store) throw new Error("useStore outside StoreProvider");
  return store;
}

/** Translates the error codes sent by the backend. */
function useErrorText() {
  const { t } = useTranslation();
  return useCallback(
    (raw: unknown) => {
      const msg = String(raw);
      if (msg.startsWith("invalid:")) return t("errors.invalid", { fields: msg.slice(8) });
      if (msg.startsWith("hotkey:")) return t("errors.hotkey", { detail: msg.slice(7) });
      if (msg === "busy") return t("errors.busy");
      if (msg === "download_busy") return t("errors.download_busy");
      return t("errors.generic", { detail: msg });
    },
    [t],
  );
}

export function StoreProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const errorText = useErrorText();

  useEffect(() => {
    const refresh = () => {
      api.settings().then(setSettings, (e: unknown) => setError(String(e)));
      api.status().then(setStatus, () => undefined);
    };
    refresh();
    const subs = [
      listen("settings-changed", refresh),
      listen("models-changed", refresh),
      listen("state", () => api.status().then(setStatus, () => undefined)),
    ];
    return () => {
      for (const s of subs) void s.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (!error) return;
    const id = window.setTimeout(() => setError(null), 6000);
    return () => window.clearTimeout(id);
  }, [error]);

  const showError = useCallback((raw: unknown) => setError(errorText(raw)), [errorText]);

  const update = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return false;
      try {
        const saved = await api.updateSettings({ ...settings, ...patch });
        setSettings(saved);
        setError(null);
        void api.status().then(setStatus, () => undefined);
        return true;
      } catch (e) {
        showError(e);
        return false;
      }
    },
    [settings, showError],
  );

  if (!settings) return null;
  return (
    <StoreContext.Provider value={{ settings, status, update, replace: setSettings, error, showError }}>
      {children}
    </StoreContext.Provider>
  );
}
