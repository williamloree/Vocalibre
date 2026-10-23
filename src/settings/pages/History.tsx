import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { api, type HistoryEntry, type ModelEntry } from "../../api";
import { useStore } from "../store";
import { ConfirmButton } from "../controls";

export function History() {
  const { t, i18n } = useTranslation();
  const { settings, showError } = useStore();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [copied, setCopied] = useState<number | null>(null);

  useEffect(() => {
    const refresh = () => api.history().then(setEntries, showError);
    refresh();
    api.models().then(setModels, () => undefined);
    const unlisten = listen("history-changed", refresh);
    return () => void unlisten.then((u) => u());
  }, [showError]);

  useEffect(() => {
    if (copied === null) return;
    const id = window.setTimeout(() => setCopied(null), 1500);
    return () => window.clearTimeout(id);
  }, [copied]);

  const date = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" }),
    [i18n.language],
  );
  const modelName = (id: string) => models.find((m) => m.id === id)?.display_name ?? id;

  return (
    <>
      {!settings.history_enabled && <p className="warning">{t("history.disabled")}</p>}
      {entries.length === 0 ? (
        <p className="empty">{t("history.empty")}</p>
      ) : (
        <>
          <div className="toolbar">
            <ConfirmButton onConfirm={() => void api.clearHistory().catch(showError)}>
              {t("history.clear")}
            </ConfirmButton>
          </div>
          <ul className="history">
            {entries.map((e) => (
              <li key={e.id}>
                <p className="history-text">{e.text}</p>
                <div className="history-meta">
                  <span>
                    {date.format(new Date(e.at))} ·{" "}
                    {t("history.duration", {
                      seconds: (e.duration_ms / 1000).toLocaleString(i18n.language, {
                        maximumFractionDigits: 1,
                      }),
                    })}{" "}
                    · {modelName(e.model_id)}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      void api.copyText(e.text).then(() => setCopied(e.id), showError)
                    }
                  >
                    {copied === e.id ? t("common.copied") : t("common.copy")}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
    </>
  );
}
