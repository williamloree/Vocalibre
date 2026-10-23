// First launch: shows what the machine offers, downloads the recommended
// model and lets the user try a dictation, without leaving the window.
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type Hardware } from "../api";
import { useStore } from "./store";
import { DownloadProgress, useDownloadError, useModels, useNum } from "./pages/Models";

type Step = "hardware" | "model" | "try";

export function Welcome({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("hardware");
  const [hw, setHw] = useState<Hardware | null>(null);

  useEffect(() => {
    api.hardware().then(setHw, () => undefined);
  }, []);

  const finish = () => {
    void api.finishOnboarding();
    onDone();
  };

  return (
    <div className="welcome">
      <header>
        <img src="/logo.png" alt="" width={40} height={40} />
        <h1>{t("welcome.title")}</h1>
        <button type="button" className="link" onClick={finish}>
          {t("welcome.skip")}
        </button>
      </header>
      <ol className="steps" aria-label={t("welcome.steps")}>
        {(["hardware", "model", "try"] as const).map((s, i) => (
          <li key={s} className={s === step ? "current" : ""}>
            {i + 1}. {t(`welcome.step.${s}`)}
          </li>
        ))}
      </ol>
      {step === "hardware" && <HardwareStep hw={hw} onNext={() => setStep("model")} />}
      {step === "model" && <ModelStep hw={hw} onNext={() => setStep("try")} />}
      {step === "try" && <TryStep onDone={finish} />}
    </div>
  );
}

function HardwareStep({ hw, onNext }: { hw: Hardware | null; onNext: () => void }) {
  const { t } = useTranslation();
  const num = useNum();
  if (!hw) return <p className="lead">{t("welcome.detecting")}</p>;
  const best = hw.gpus.find((g) => !g.integrated) ?? hw.gpus[0];
  return (
    <section className="section">
      <p className="lead">{t("welcome.intro")}</p>
      {best ? (
        <p>
          {t("welcome.gpu_found")} <strong>{best.name}</strong>
          {best.vram_mb > 0 && ` (${t("models.gb", { value: num(best.vram_mb / 1000, 1) })})`}
        </p>
      ) : (
        <p>{t("welcome.no_gpu", { threads: hw.cpu_threads })}</p>
      )}
      <p className="hint">{t("welcome.private")}</p>
      <div className="actions">
        <button type="button" className="primary" onClick={onNext}>
          {t("welcome.next")}
        </button>
      </div>
    </section>
  );
}

function ModelStep({ hw, onNext }: { hw: Hardware | null; onNext: () => void }) {
  const { t } = useTranslation();
  const { status, showError } = useStore();
  const { models, progress, failed, download, anyDownload } = useModels();
  const downloadError = useDownloadError();
  const num = useNum();
  const recommended = models.find((m) => hw && m.recommended_for.includes(hw.class));
  const [chosen, setChosen] = useState<string | null>(null);
  const id = chosen ?? recommended?.id ?? models[0]?.id;
  const model = models.find((m) => m.id === id);
  const ready = status?.model != null && status.state.kind === "idle";

  return (
    <section className="section">
      <p className="lead">{t("welcome.model_intro")}</p>
      <div className="choices" role="radiogroup">
        {models.map((m) => (
          <label key={m.id} className={m.id === id ? "choice selected" : "choice"}>
            <input
              type="radio"
              name="model"
              checked={m.id === id}
              disabled={anyDownload}
              onChange={() => setChosen(m.id)}
            />
            <span className="choice-name">
              {m.display_name}
              {m.id === recommended?.id && <span className="badge accent">{t("welcome.recommended")}</span>}
              {m.installed && <span className="badge">{t("models.installed")}</span>}
            </span>
            <span className="hint">
              {t("models.mb", { value: num(m.file.size / 1e6, 0) })} ·{" "}
              {m.languages.includes("*") ? t("models.all_languages") : m.languages.join(", ")}
            </span>
          </label>
        ))}
      </div>
      {model && progress[model.id] && <DownloadProgress p={progress[model.id]} num={num} />}
      {model && failed[model.id] && <p className="warning">{downloadError(failed[model.id])}</p>}
      {status?.state.kind === "loading_model" && <p className="hint">{t("welcome.loading")}</p>}
      {ready && status.model && (
        <p>
          {t("welcome.model_ready")}{" "}
          <strong>
            {status.model.on_gpu ? t("models.on_gpu") : t("models.on_cpu")}
            {status.model.gpu_name && ` (${status.model.gpu_name})`}
          </strong>
        </p>
      )}
      <div className="actions">
        {model && !model.installed && !progress[model.id] && (
          <button type="button" className="primary" disabled={anyDownload} onClick={() => download(model.id)}>
            {t("models.download", { size: t("models.mb", { value: num(model.file.size / 1e6, 0) }) })}
          </button>
        )}
        {model?.installed && !model.active && (
          <button type="button" onClick={() => void api.selectModel(model.id).catch(showError)}>
            {t("models.activate")}
          </button>
        )}
        <button type="button" className={ready ? "primary" : ""} disabled={!ready} onClick={onNext}>
          {t("welcome.next")}
        </button>
      </div>
    </section>
  );
}

function TryStep({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const { settings: s, status } = useStore();
  const kind = status?.state.kind;
  return (
    <section className="section">
      <p className="lead">{t("welcome.try_intro")}</p>
      <ul className="keys">
        {s.hotkey && (
          <li>
            <kbd>{s.hotkey}</kbd> {t("welcome.try_toggle")}
          </li>
        )}
        {s.ptt_hotkey && (
          <li>
            <kbd>{s.ptt_hotkey}</kbd> {t("welcome.try_ptt")}
          </li>
        )}
      </ul>
      <textarea
        className="try"
        // The dictation is pasted where the cursor is: here.
        autoFocus
        rows={4}
        placeholder={t("welcome.try_placeholder")}
      />
      <p className="hint" aria-live="polite">
        {kind === "recording" ? t("state.recording") : kind === "transcribing" ? t("state.transcribing") : " "}
      </p>
      <p className="hint">{t("welcome.tray")}</p>
      <div className="actions">
        <button type="button" className="primary" onClick={onDone}>
          {t("welcome.done")}
        </button>
      </div>
    </section>
  );
}
