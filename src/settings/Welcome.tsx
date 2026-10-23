// First launch: shows what the machine offers, downloads the recommended
// model and lets the user try a dictation, without leaving the window.
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api, type Hardware } from "../api";
import { useStore } from "./store";
import { DownloadProgress, useDownloadError, useModels, useNum } from "./pages/Models";

const STEPS = ["hardware", "model", "try"] as const;
type Step = (typeof STEPS)[number];

export function Welcome({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation();
  const [step, setStep] = useState<Step>("hardware");
  const [hw, setHw] = useState<Hardware | null>(null);
  const current = STEPS.indexOf(step);

  useEffect(() => {
    api.hardware().then(setHw, () => undefined);
  }, []);

  const finish = () => {
    void api.finishOnboarding();
    onDone();
  };
  const back = current > 0 ? () => setStep(STEPS[current - 1]) : undefined;

  return (
    <div className="welcome">
      <button type="button" className="link welcome-skip" onClick={finish}>
        {t("welcome.skip")}
      </button>
      <div className="welcome-inner">
        <header className="welcome-head">
          <h1>
            <img src="/logo.png" alt="" width={40} height={40} />
            {t("welcome.title")}
          </h1>
          {step === "hardware" && <p>{t("welcome.intro")}</p>}
        </header>
        <ol className="stepper" aria-label={t("welcome.steps")}>
          {STEPS.map((s, i) => (
            <li
              key={s}
              className={i < current ? "done" : i === current ? "current" : ""}
              aria-current={i === current ? "step" : undefined}
            >
              <span className="stepper-dot">{i < current ? "✓" : i + 1}</span>
              {t(`welcome.step.${s}`)}
            </li>
          ))}
        </ol>
        {step === "hardware" && <HardwareStep hw={hw} onNext={() => setStep("model")} />}
        {step === "model" && <ModelStep hw={hw} onBack={back} onNext={() => setStep("try")} />}
        {step === "try" && <TryStep onBack={back} onDone={finish} />}
      </div>
    </div>
  );
}

/** One step: a titled card with the navigation at the bottom. */
function Card({ title, actions, children }: { title: string; actions: ReactNode; children: ReactNode }) {
  return (
    <section className="welcome-card">
      <h2>{title}</h2>
      <div className="welcome-body">{children}</div>
      <footer className="welcome-actions">{actions}</footer>
    </section>
  );
}

function BackButton({ onBack }: { onBack?: () => void }) {
  const { t } = useTranslation();
  return onBack ? (
    <button type="button" className="link" onClick={onBack}>
      {t("welcome.back")}
    </button>
  ) : (
    <span />
  );
}

function HardwareStep({ hw, onNext }: { hw: Hardware | null; onNext: () => void }) {
  const { t } = useTranslation();
  const num = useNum();
  const best = hw && (hw.gpus.find((g) => !g.integrated) ?? hw.gpus[0]);
  return (
    <Card
      title={t("welcome.hardware_title")}
      actions={
        <>
          <span />
          <button type="button" className="primary" disabled={!hw} onClick={onNext}>
            {t("welcome.next")}
          </button>
        </>
      }
    >
      {hw ? (
        <div className="tiles">
          <div className={best ? "tile active" : "tile"}>
            <span className="tile-label">{t("welcome.gpu_label")}</span>
            <strong>{best ? best.name : t("welcome.none")}</strong>
            {best && (
              <span className="hint">
                {best.vram_mb > 0 && `${t("models.gb", { value: num(best.vram_mb / 1000, 1) })} · `}
                {t("welcome.does_work")}
              </span>
            )}
          </div>
          <div className={best ? "tile" : "tile active"}>
            <span className="tile-label">{t("welcome.cpu_label")}</span>
            <strong>{t("welcome.threads", { count: hw.cpu_threads })}</strong>
            {!best && <span className="hint">{t("welcome.does_work")}</span>}
          </div>
        </div>
      ) : (
        <p className="hint">{t("welcome.detecting")}</p>
      )}
      <p className="hint">{t("welcome.private")}</p>
    </Card>
  );
}

function ModelStep({ hw, onBack, onNext }: { hw: Hardware | null; onBack?: () => void; onNext: () => void }) {
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
    <Card
      title={t("welcome.model_title")}
      actions={
        <>
          <BackButton onBack={onBack} />
          <div className="actions">
            {model && !model.installed && !progress[model.id] && (
              <button type="button" className="primary" disabled={anyDownload} onClick={() => download(model.id)}>
                {t("models.download", { size: t("models.mb", { value: num(model.file.size / 1e6, 0) }) })}
              </button>
            )}
            {model?.installed && !model.active && (
              <button type="button" className="primary" onClick={() => void api.selectModel(model.id).catch(showError)}>
                {t("models.activate")}
              </button>
            )}
            {(ready || model?.active) && (
              <button type="button" className="primary" disabled={!ready} onClick={onNext}>
                {t("welcome.next")}
              </button>
            )}
          </div>
        </>
      }
    >
      <p className="hint">{t("welcome.model_intro")}</p>
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
        <p className="ready">
          {t("welcome.model_ready")}{" "}
          <strong>
            {status.model.on_gpu ? t("models.on_gpu") : t("models.on_cpu")}
            {status.model.gpu_name && ` (${status.model.gpu_name})`}
          </strong>
        </p>
      )}
    </Card>
  );
}

function TryStep({ onBack, onDone }: { onBack?: () => void; onDone: () => void }) {
  const { t } = useTranslation();
  const { settings: s, status } = useStore();
  const kind = status?.state.kind;
  return (
    <Card
      title={t("welcome.try_title")}
      actions={
        <>
          <BackButton onBack={onBack} />
          <button type="button" className="primary" onClick={onDone}>
            {t("welcome.done")}
          </button>
        </>
      }
    >
      <p className="hint">{t("welcome.try_intro")}</p>
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
        {kind === "recording" ? t("state.recording") : kind === "transcribing" ? t("state.transcribing") : " "}
      </p>
      <p className="hint">{t("welcome.tray")}</p>
    </Card>
  );
}
