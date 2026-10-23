import { useTranslation } from "react-i18next";
import type { Settings } from "../../api";
import { useStore } from "../store";
import { NumberField, Radios, Section, Toggle } from "../controls";

export function Injection() {
  const { t } = useTranslation();
  const { settings: s, update } = useStore();
  return (
    <>
      <Section>
        <Radios<Settings["injection_mode"]>
          label={t("injection.mode")}
          value={s.injection_mode}
          options={[
            { value: "paste", label: t("injection.paste") },
            { value: "type", label: t("injection.type") },
          ]}
          onChange={(injection_mode) => void update({ injection_mode })}
        />
        {s.injection_mode === "paste" && !s.keep_in_clipboard && (
          <NumberField
            label={t("injection.restore_delay")}
            value={s.restore_delay_ms}
            min={0}
            max={5000}
            onChange={(restore_delay_ms) => void update({ restore_delay_ms })}
          />
        )}
        <Toggle
          label={t("injection.keep_in_clipboard")}
          checked={s.keep_in_clipboard}
          onChange={(keep_in_clipboard) => void update({ keep_in_clipboard })}
        />
      </Section>
      <p className="hint">{t("injection.admin_hint")}</p>
    </>
  );
}
