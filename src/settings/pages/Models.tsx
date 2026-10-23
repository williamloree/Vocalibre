import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { api, type BenchResult, type DownloadEvent, type ModelEntry } from "../../api";
import { useStore } from "../store";
import { ConfirmButton, Section } from "../controls";

function Dots({ value }: { value: number }) {
  return (
    <span className="dots" aria-label={`${value}/5`}>
      {[1, 2, 3, 4, 5].map((i) => (
        <span key={i} className={i <= value ? "on" : ""} />
      ))}
    </span>
  );
}

/** Progress of the running download, with what is needed for the speed. */
type Progress = { downloaded: number; total: number; since: number; from: number };

/** Translated text for a download error code (`code:detail`). */
function useDownloadError() {
  const { t } = useTranslation();
  return (raw: string) => {
    const [code, ...rest] = raw.split(":");
    const detail = rest.join(":");
    switch (code) {
      case "network":
        return t("models.download_error.network", { detail });
      case "http":
        return t("models.download_error.http", { detail });
      case "no_space":
        return t("models.download_error.no_space", { detail });
      case "checksum":
        return t("models.download_error.checksum");
      default:
        return t("models.download_error.io", { detail: detail || raw });
    }
  };
}

export function Models() {
  const { t, i18n } = useTranslation();
  const { status, showError } = useStore();
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [bench, setBench] = useState<BenchResult | "running" | null>(null);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [failed, setFailed] = useState<Record<string, string>>({});
  const downloadError = useDownloadError();

  useEffect(() => {
    const refresh = () =>
      api.models().then((list) => {
        setModels(list);
        // A download started before this window opened.
        for (const m of list) {
          if (m.download) {
            const [downloaded, total] = m.download;
            setProgress((p) => ({ ...p, [m.id]: p[m.id] ?? { downloaded, total, since: Date.now(), from: downloaded } }));
          }
        }
      }, showError);
    refresh();
    const subs = [
      listen("models-changed", refresh),
      listen<DownloadEvent>("download", ({ payload: e }) => {
        if (e.done) {
          setProgress(({ [e.model_id]: _, ...rest }) => rest);
          if (e.error && e.error !== "cancelled") setFailed((f) => ({ ...f, [e.model_id]: e.error ?? "" }));
          return;
        }
        setProgress((p) => {
          const prev = p[e.model_id];
          const start = prev ?? { since: Date.now(), from: e.downloaded };
          return { ...p, [e.model_id]: { ...start, downloaded: e.downloaded, total: e.total } };
        });
      }),
    ];
    return () => {
      for (const s of subs) void s.then((u) => u());
    };
  }, [showError]);

  const download = (id: string) => {
    setFailed(({ [id]: _, ...rest }) => rest);
    setProgress((p) => ({ ...p, [id]: { downloaded: 0, total: 0, since: Date.now(), from: 0 } }));
    api.downloadModel(id).catch((e: unknown) => {
      setProgress(({ [id]: _, ...rest }) => rest);
      showError(e);
    });
  };
  const anyDownload = Object.keys(progress).length > 0;

  const num = (n: number, digits = 2) =>
    n.toLocaleString(i18n.language, { maximumFractionDigits: digits });
  const loading = status?.state.kind === "loading_model";
  const runBench = () => {
    setBench("running");
    api.benchmark().then(setBench, (e: unknown) => {
      setBench(null);
      showError(e);
    });
  };

  return (
    <>
      <Section>
        <p className="lead">
          {t("models.acceleration")} :{" "}
          {status?.model ? (
            <strong>{status.model.on_gpu ? t("models.on_gpu") : t("models.on_cpu")}</strong>
          ) : (
            t("models.none_loaded")
          )}
        </p>
        {status?.model?.fell_back_to_cpu && <p className="warning">{t("models.fell_back")}</p>}
      </Section>
      <div className="cards">
        {models.map((m) => (
          <article key={m.id} className={m.active ? "card active" : "card"}>
            <header>
              <h3>{m.display_name}</h3>
              <div className="badges">
                {m.active && <span className="badge accent">{t("models.active")}</span>}
                {m.recommended_for.map((c) => (
                  <span key={c} className="badge">
                    {t(`models.recommended_for.${c}`)}
                  </span>
                ))}
                {m.installed && !m.active && <span className="badge">{t("models.installed")}</span>}
              </div>
            </header>
            <dl>
              <dt>{t("models.size")}</dt>
              <dd>{t("models.mb", { value: num(m.file.size / 1e6, 0) })}</dd>
              <dt>{t("models.ram")}</dt>
              <dd>{t("models.gb", { value: num(m.ram_mb / 1000, 1) })}</dd>
              <dt>{t("models.speed")}</dt>
              <dd>
                <Dots value={m.speed} />
              </dd>
              <dt>{t("models.accuracy")}</dt>
              <dd>
                <Dots value={m.accuracy} />
              </dd>
              <dt>{t("models.languages")}</dt>
              <dd>{m.languages.includes("*") ? t("models.all_languages") : m.languages.join(", ")}</dd>
            </dl>
            {m.installed ? (
              <div className="actions">
                {!m.active && (
                  <button type="button" disabled={loading} onClick={() => void api.selectModel(m.id).catch(showError)}>
                    {t("models.activate")}
                  </button>
                )}
                {m.active && (
                  <button type="button" disabled={bench === "running"} onClick={runBench}>
                    {bench === "running" ? t("models.bench_running") : t("models.bench")}
                  </button>
                )}
                <ConfirmButton onConfirm={() => void api.deleteModel(m.id).catch(showError)}>
                  {t("models.delete")}
                </ConfirmButton>
              </div>
            ) : progress[m.id] ? (
              <DownloadProgress p={progress[m.id]} num={num} />
            ) : (
              <>
                <div className="actions">
                  <button type="button" disabled={anyDownload} onClick={() => download(m.id)}>
                    {t("models.download", { size: t("models.mb", { value: num(m.file.size / 1e6, 0) }) })}
                  </button>
                </div>
                {failed[m.id] && <p className="warning">{downloadError(failed[m.id])}</p>}
                <p className="hint">{t("models.not_installed", { file: m.file.file_name })}</p>
              </>
            )}
            {m.active && bench && bench !== "running" && (
              <p className="bench">
                {t("models.bench_result", {
                  audio: num(bench.audio_ms / 1000, 1),
                  inference: num(bench.inference_ms / 1000),
                })}
                <br />
                <q>{bench.text}</q>
              </p>
            )}
          </article>
        ))}
      </div>
      <Section title={t("models.folder")}>
        <p className="path">{status?.models_dir}</p>
        <p className="hint">{t("models.vad_hint", { file: status?.vad_file })}</p>
        <button type="button" onClick={() => void api.openModelsDir().catch(showError)}>
          {t("common.open_folder")}
        </button>
      </Section>
    </>
  );
}

function DownloadProgress({ p, num }: { p: Progress; num: (n: number, digits?: number) => string }) {
  const { t } = useTranslation();
  const seconds = (Date.now() - p.since) / 1000;
  const speed = seconds > 1 ? (p.downloaded - p.from) / seconds : 0;
  const left = speed > 0 ? Math.ceil((p.total - p.downloaded) / speed) : null;
  return (
    <div className="download">
      <progress value={p.downloaded} max={p.total || 1} />
      <p className="hint">
        {t("models.download_progress", {
          done: num(p.downloaded / 1e6, 0),
          total: num(p.total / 1e6, 0),
        })}
        {speed > 0 && ` · ${t("models.download_speed", { speed: num(speed / 1e6, 1) })}`}
        {left !== null && ` · ${t("models.download_left", { count: left })}`}
      </p>
      <button type="button" className="link" onClick={() => void api.cancelDownload()}>
        {t("common.cancel")}
      </button>
    </div>
  );
}
