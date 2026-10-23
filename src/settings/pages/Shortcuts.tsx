import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import { api } from "../../api";
import { useStore } from "../store";
import { Row, Section } from "../controls";

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
 * Captures the next key combination. The shortcuts are released meanwhile,
 * otherwise pressing one would start a dictation instead. `onClear` adds a
 * button turning the shortcut off.
 */
function ShortcutInput({
  label,
  hint,
  value,
  requireModifier,
  onChange,
  onClear,
}: {
  label: string;
  hint?: ReactNode;
  value: string | null;
  requireModifier: boolean;
  onChange: (combo: string) => void;
  onClear?: () => void;
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
    <Row label={label} hint={problem ?? hint}>
      {capturing ? (
        <span className="capture">
          <kbd className="pending">{t("shortcuts.press")}</kbd>
          <button type="button" className="link" onClick={() => setCapturing(false)}>
            {t("common.cancel")}
          </button>
        </span>
      ) : (
        <span className="capture">
          {value ? <kbd>{value}</kbd> : <span className="hint">{t("shortcuts.none")}</span>}
          <button type="button" onClick={() => setCapturing(true)}>
            {value ? t("shortcuts.change") : t("shortcuts.set")}
          </button>
          {value && onClear && (
            <button type="button" className="link" onClick={onClear}>
              {t("shortcuts.disable")}
            </button>
          )}
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
        <ShortcutInput
          label={t("shortcuts.hotkey")}
          hint={
            s.hotkey?.toLowerCase() === "ctrl+space" ? t("shortcuts.ctrl_space_hint") : t("shortcuts.hotkey_hint")
          }
          value={s.hotkey}
          requireModifier
          onChange={(hotkey) => void update({ hotkey })}
          onClear={s.ptt_hotkey ? () => void update({ hotkey: null }) : undefined}
        />
        <ShortcutInput
          label={t("shortcuts.ptt_hotkey")}
          hint={t("shortcuts.ptt_hint")}
          value={s.ptt_hotkey}
          requireModifier
          onChange={(ptt_hotkey) => void update({ ptt_hotkey })}
          onClear={s.hotkey ? () => void update({ ptt_hotkey: null }) : undefined}
        />
        {status?.hotkey_error && (
          <p className="warning">{t("errors.hotkey", { detail: status.hotkey_error })}</p>
        )}
      </Section>
      <Section>
        <ShortcutInput
          label={t("shortcuts.paste_last")}
          hint={t("shortcuts.paste_last_hint")}
          value={s.paste_last_hotkey}
          requireModifier
          onChange={(paste_last_hotkey) => void update({ paste_last_hotkey })}
          onClear={() => void update({ paste_last_hotkey: null })}
        />
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
