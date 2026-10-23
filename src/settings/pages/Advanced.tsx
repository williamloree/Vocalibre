import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type AppInfo } from "../../api";
import { useStore } from "../store";
import { ConfirmButton, Section, Toggle } from "../controls";

export function Advanced() {
  const { t } = useTranslation();
  const { settings: s, status, update, replace, showError } = useStore();
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    api.appInfo().then(setInfo, showError);
  }, [showError]);

  const chooseModelsDir = async () => {
    const dir = await open({ directory: true, defaultPath: status?.models_dir });
    if (typeof dir === "string") void update({ models_dir: dir });
  };

  return (
    <>
      <Section title={t("advanced.models_dir")}>
        <p className="path">{status?.models_dir}</p>
        <div className="actions">
          <button type="button" onClick={() => void chooseModelsDir().catch(showError)}>
            {t("advanced.choose")}
          </button>
          {s.models_dir !== null && (
            <button type="button" onClick={() => void update({ models_dir: null })}>
              {t("advanced.default_dir")}
            </button>
          )}
        </div>
      </Section>
      <Section title={t("advanced.logs")}>
        {info && <p className="path">{info.log_dir}</p>}
        <button type="button" onClick={() => void api.openLogsDir().catch(showError)}>
          {t("advanced.open_logs")}
        </button>
        <Toggle
          label={t("advanced.debug_log_text")}
          hint={<span className={s.debug_log_text ? "warning" : ""}>{t("advanced.debug_log_text_hint")}</span>}
          checked={s.debug_log_text}
          onChange={(debug_log_text) => void update({ debug_log_text })}
        />
      </Section>
      <Section title={t("advanced.config_dir")}>
        {info && <p className="path">{info.config_dir}</p>}
        <p className="hint">{t("advanced.reset_hint")}</p>
        <ConfirmButton onConfirm={() => void api.resetSettings().then(replace, showError)}>
          {t("advanced.reset")}
        </ConfirmButton>
      </Section>
    </>
  );
}
