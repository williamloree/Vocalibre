import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Replacement } from "../../api";
import { useStore } from "../store";
import { Section, Select, Toggle } from "../controls";

// Must match `LANGUAGES` in src-tauri/src/settings/mod.rs.
const LANGUAGES = [
  "fr", "en", "de", "es", "it", "pt", "nl", "pl", "ru", "uk", "cs", "sv", "da", "no", "fi", "el",
  "tr", "ro", "hu", "bg", "hr", "sk", "sl", "lt", "lv", "et", "ca", "eu", "gl", "ar", "he", "fa",
  "hi", "bn", "ja", "ko", "zh", "vi", "th", "id", "ms",
];
const VOCABULARY_MAX = 2000;

/** Replacements are edited with `\n` standing for a line break. */
const toDisplay = (s: string) => s.replace(/\n/g, "\\n");
const fromDisplay = (s: string) => s.replace(/\\n/g, "\n");

type Rule = { from: string; to: string };

export function Transcription() {
  const { t, i18n } = useTranslation();
  const { settings: s, update } = useStore();
  const [vocabulary, setVocabulary] = useState(s.vocabulary);
  const [rows, setRows] = useState<Rule[]>(() =>
    s.replacements.map((r) => ({ from: r.from, to: toDisplay(r.to) })),
  );
  useEffect(() => setVocabulary(s.vocabulary), [s.vocabulary]);

  const languages = useMemo(() => {
    const names = new Intl.DisplayNames([i18n.language], { type: "language" });
    return LANGUAGES.map((code) => ({ value: code, label: names.of(code) ?? code })).sort((a, b) =>
      a.label.localeCompare(b.label, i18n.language),
    );
  }, [i18n.language]);

  const saveRows = (next: Rule[]) => {
    const replacements: Replacement[] = next
      .filter((r) => r.from.trim() !== "")
      .map((r) => ({ from: r.from.trim(), to: fromDisplay(r.to) }));
    void update({ replacements });
  };
  const edit = (i: number, patch: Partial<Rule>) =>
    setRows(rows.map((x, j) => (j === i ? { ...x, ...patch } : x)));

  return (
    <>
      <Section>
        <Select
          label={t("transcription.language")}
          value={s.language}
          options={[{ value: "auto", label: t("transcription.language_auto") }, ...languages]}
          onChange={(language) => void update({ language })}
        />
        <div className="stack">
          <label htmlFor="vocabulary">{t("transcription.vocabulary")}</label>
          <textarea
            id="vocabulary"
            rows={3}
            maxLength={VOCABULARY_MAX}
            value={vocabulary}
            onChange={(e) => setVocabulary(e.target.value)}
            onBlur={() => vocabulary !== s.vocabulary && void update({ vocabulary })}
          />
          <p className="hint">{t("transcription.vocabulary_hint", { count: vocabulary.length })}</p>
        </div>
      </Section>
      <Section title={t("transcription.replacements")}>
        <p className="hint">
          {t("transcription.replacements_hint")} {t("transcription.newline")}
        </p>
        {rows.length > 0 && (
          <table className="replacements">
            <thead>
              <tr>
                <th>{t("transcription.from")}</th>
                <th>{t("transcription.to")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i}>
                  <td>
                    <input
                      aria-label={t("transcription.from")}
                      value={r.from}
                      onChange={(e) => edit(i, { from: e.target.value })}
                      onBlur={() => saveRows(rows)}
                    />
                  </td>
                  <td>
                    <input
                      aria-label={t("transcription.to")}
                      value={r.to}
                      onChange={(e) => edit(i, { to: e.target.value })}
                      onBlur={() => saveRows(rows)}
                    />
                  </td>
                  <td>
                    <button
                      type="button"
                      className="link"
                      onClick={() => {
                        const next = rows.filter((_, j) => j !== i);
                        setRows(next);
                        saveRows(next);
                      }}
                    >
                      {t("transcription.remove")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <button type="button" onClick={() => setRows([...rows, { from: "", to: "" }])}>
          {t("transcription.add")}
        </button>
      </Section>
      <Section>
        <Toggle
          label={t("transcription.capitalize_first")}
          checked={s.capitalize_first}
          onChange={(capitalize_first) => void update({ capitalize_first })}
        />
        <Toggle
          label={t("transcription.strip_final_period")}
          checked={s.strip_final_period}
          onChange={(strip_final_period) => void update({ strip_final_period })}
        />
        <Toggle
          label={t("transcription.auto_space")}
          hint={t("transcription.auto_space_hint")}
          checked={s.auto_space}
          onChange={(auto_space) => void update({ auto_space })}
        />
        <Select
          label={t("transcription.beam_size")}
          hint={t("transcription.beam_hint")}
          value={String(s.beam_size)}
          options={[
            { value: "0", label: t("common.auto") },
            ...Array.from({ length: 10 }, (_, i) => ({ value: String(i + 1), label: String(i + 1) })),
          ]}
          onChange={(v) => void update({ beam_size: Number(v) })}
        />
      </Section>
    </>
  );
}
