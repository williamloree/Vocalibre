import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type MicInfo, type Settings } from "../../api";
import { useStore } from "../store";
import { NumberField, Radios, Section, Select, Toggle } from "../controls";

/** Microphones come and go: the list is refreshed while the page is open. */
const MIC_REFRESH_MS = 2000;

export function Hardware() {
  const { t } = useTranslation();
  const { settings: s, update } = useStore();
  const [mics, setMics] = useState<MicInfo[]>([]);

  useEffect(() => {
    const refresh = () => api.microphones().then(setMics, () => undefined);
    refresh();
    const id = window.setInterval(refresh, MIC_REFRESH_MS);
    return () => window.clearInterval(id);
  }, []);

  const options = [
    { value: "", label: t("hardware.microphone_default") },
    ...mics.map((m) => ({ value: m.id, label: m.name })),
  ];
  if (s.microphone && !mics.some((m) => m.id === s.microphone)) {
    options.push({
      value: s.microphone,
      label: t("hardware.microphone_missing", { name: s.microphone }),
    });
  }

  return (
    <>
      <Section>
        <Select
          label={t("hardware.microphone")}
          value={s.microphone ?? ""}
          options={options}
          onChange={(v) => void update({ microphone: v === "" ? null : v })}
        />
      </Section>
      <Section>
        <Radios<Settings["acceleration"]>
          label={t("hardware.acceleration")}
          value={s.acceleration}
          options={[
            { value: "auto", label: t("hardware.acceleration_auto") },
            { value: "gpu", label: t("hardware.acceleration_gpu") },
            { value: "cpu", label: t("hardware.acceleration_cpu") },
          ]}
          onChange={(acceleration) => void update({ acceleration })}
        />
        <NumberField
          label={t("hardware.threads")}
          hint={t("hardware.threads_hint")}
          value={s.threads}
          min={0}
          max={64}
          onChange={(threads) => void update({ threads })}
        />
      </Section>
      <Section>
        <NumberField
          label={t("hardware.max_duration")}
          value={s.max_duration_s}
          min={5}
          max={3600}
          onChange={(max_duration_s) => void update({ max_duration_s })}
        />
        <Toggle
          label={t("hardware.silence_auto_stop")}
          checked={s.silence_auto_stop}
          onChange={(silence_auto_stop) => void update({ silence_auto_stop })}
        />
        {s.silence_auto_stop && (
          <NumberField
            label={t("hardware.silence_timeout")}
            value={s.silence_timeout_s}
            min={1}
            max={60}
            onChange={(silence_timeout_s) => void update({ silence_timeout_s })}
          />
        )}
      </Section>
    </>
  );
}
