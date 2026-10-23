import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type RecordingMode } from "../../api";
import { useStore } from "../store";
import { Radios, Row, Section } from "../controls";

const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "AltLeft", "AltRight",
  "ShiftLeft", "ShiftRight", "MetaLeft", "MetaRight",
]);

/** `KeyboardEvent.code` → key name understood by the global shortcut parser. */
function keyName(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

/** "Ctrl+Shift+Space"; `null` while only modifiers are held. */
function comboFrom(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.has(e.code)) return null;
  const parts = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Super");
  parts.push(keyName(e.code));
  return parts.join("+");
}

const isFunctionKey = (combo: string) => /^F([1-9]|1\d|2[0-4])$/.test(combo);

/**
 * Captures the next key combination. The dictation shortcut is released
 * meanwhile, otherwise pressing it would start a dictation instead.
 */
function ShortcutInput({
  label,
  value,
  requireModifier,
  onChange,
}: {
  label: string;
  value: string;
  requireModifier: boolean;
  onChange: (combo: string) => void;
}) {
  const { t } = useTranslation();
  const [capturing, setCapturing] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    if (!capturing) return;
    void api.suspendHotkey(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const combo = comboFrom(e);
      if (!combo) return;
      if (requireModifier && !combo.includes("+") && !isFunctionKey(combo)) {
        setProblem(t("shortcuts.need_modifier"));
        return;
      }
      setProblem(null);
      setCapturing(false);
      onChange(combo);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      void api.suspendHotkey(false);
    };
  }, [capturing, requireModifier, onChange, t]);

  return (
    <Row label={label} hint={problem ?? undefined}>
      {capturing ? (
        <span className="capture">
          <kbd className="pending">{t("shortcuts.press")}</kbd>
          <button type="button" className="link" onClick={() => setCapturing(false)}>
            {t("common.cancel")}
          </button>
        </span>
      ) : (
        <span className="capture">
          <kbd>{value}</kbd>
          <button type="button" onClick={() => setCapturing(true)}>
            {t("shortcuts.change")}
          </button>
        </span>
      )}
    </Row>
  );
}

export function Shortcuts() {
  const { t } = useTranslation();
  const { settings: s, status, update } = useStore();
  return (
    <>
      <Section>
        <Radios<RecordingMode>
          label={t("shortcuts.mode")}
          value={s.recording_mode}
          options={[
            { value: "toggle", label: t("shortcuts.toggle") },
            { value: "push_to_talk", label: t("shortcuts.push_to_talk") },
          ]}
          onChange={(recording_mode) => void update({ recording_mode })}
        />
      </Section>
      <Section>
        <ShortcutInput
          label={t("shortcuts.hotkey")}
          value={s.hotkey}
          requireModifier
          onChange={(hotkey) => void update({ hotkey })}
        />
        {status?.hotkey_error && (
          <p className="warning">{t("errors.hotkey", { detail: status.hotkey_error })}</p>
        )}
        {s.hotkey.toLowerCase() === "ctrl+space" && <p className="hint">{t("shortcuts.ctrl_space_hint")}</p>}
        <ShortcutInput
          label={t("shortcuts.cancel_key")}
          value={s.cancel_key}
          requireModifier={false}
          onChange={(cancel_key) => void update({ cancel_key })}
        />
      </Section>
    </>
  );
}
