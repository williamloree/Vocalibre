import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { api, type BenchResult, type ModelEntry } from "../../api";
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

export function Models() {
  const { t, i18n } = useTranslation();
  const { status, showError } = useStore();
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [bench, setBench] = useState<BenchResult | "running" | null>(null);

  useEffect(() => {
    const refresh = () => api.models().then(setModels, showError);
    refresh();
    const unlisten = listen("models-changed", refresh);
    return () => void unlisten.then((u) => u());
  }, [showError]);

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
            ) : (
              <p className="hint">{t("models.not_installed", { file: m.file.file_name })}</p>
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
