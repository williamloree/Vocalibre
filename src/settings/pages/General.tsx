import { useTranslation } from "react-i18next";
import { useStore } from "../store";
import { Row, Section, Select, Toggle } from "../controls";
import type { Settings } from "../../api";

export function General() {
  const { t } = useTranslation();
  const { settings: s, update } = useStore();
  return (
    <>
      <Section>
        <Select<Settings["ui_language"]>
          label={t("general.ui_language")}
          value={s.ui_language}
          options={[
            { value: "system", label: t("general.ui_language_system") },
            { value: "fr", label: "Français" },
            { value: "en", label: "English" },
          ]}
          onChange={(ui_language) => void update({ ui_language })}
        />
        <Toggle
          label={t("general.autostart")}
          hint={import.meta.env.DEV ? t("general.autostart_hint") : undefined}
          checked={s.autostart}
          onChange={(autostart) => void update({ autostart })}
        />
      </Section>
      <Section>
        <Toggle
          label={t("general.sounds")}
          hint={t("general.sounds_hint")}
          checked={s.sounds_enabled}
          onChange={(sounds_enabled) => void update({ sounds_enabled })}
        />
        {s.sounds_enabled && (
          <Row label={t("general.volume")} htmlFor="volume">
            <input
              id="volume"
              type="range"
              min={0}
              max={100}
              step={5}
              defaultValue={Math.round(s.sound_volume * 100)}
              onPointerUp={(e) => void update({ sound_volume: Number(e.currentTarget.value) / 100 })}
              onKeyUp={(e) => void update({ sound_volume: Number(e.currentTarget.value) / 100 })}
            />
          </Row>
        )}
        <Toggle
          label={t("general.overlay")}
          hint={t("general.overlay_hint")}
          checked={s.overlay_enabled}
          onChange={(overlay_enabled) => void update({ overlay_enabled })}
        />
        <Toggle
          label={t("general.history")}
          hint={t("general.history_hint")}
          checked={s.history_enabled}
          onChange={(history_enabled) => void update({ history_enabled })}
        />
        <Toggle
          label={t("general.check_updates")}
          hint={t("general.check_updates_hint")}
          checked={s.check_updates}
          onChange={(check_updates) => void update({ check_updates })}
        />
      </Section>
    </>
  );
}
