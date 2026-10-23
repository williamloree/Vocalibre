import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../../api";
import { Section } from "../controls";

const REPO = "https://github.com/williamloree/Vocalibre";

const THIRD_PARTY = [
  { name: "whisper.cpp", license: "MIT", url: "https://github.com/ggml-org/whisper.cpp" },
  { name: "OpenAI Whisper", license: "MIT", url: "https://github.com/openai/whisper" },
  { name: "Silero VAD", license: "MIT", url: "https://github.com/snakers4/silero-vad" },
  { name: "whisper-rs", license: "Unlicense", url: "https://codeberg.org/tazz4843/whisper-rs" },
  { name: "Tauri", license: "MIT / Apache-2.0", url: "https://tauri.app" },
  { name: "cpal", license: "Apache-2.0", url: "https://github.com/RustAudio/cpal" },
  { name: "rodio", license: "MIT / Apache-2.0", url: "https://github.com/RustAudio/rodio" },
  { name: "rubato", license: "MIT / Apache-2.0", url: "https://github.com/HEnquist/rubato" },
  { name: "enigo", license: "MIT", url: "https://github.com/enigo-rs/enigo" },
  { name: "arboard", license: "MIT / Apache-2.0", url: "https://github.com/1Password/arboard" },
  { name: "React", license: "MIT", url: "https://react.dev" },
  { name: "i18next", license: "MIT", url: "https://www.i18next.com" },
];

function Link({ href, children }: { href: string; children: string }) {
  return (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        void openUrl(href);
      }}
    >
      {children}
    </a>
  );
}

export function About() {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  useEffect(() => {
    api.appInfo().then((i) => setVersion(i.version), () => undefined);
  }, []);
  return (
    <>
      <Section>
        <div className="about">
          <img src="/logo.png" alt="" width={64} height={64} />
          <div>
            <p className="lead">
              <strong>Vocalibre</strong> — {t("about.tagline")}
            </p>
            <p>{t("about.version", { version })}</p>
            <p>
              {t("about.license")} <Link href={REPO}>{t("about.source")}</Link>
            </p>
          </div>
        </div>
      </Section>
      <Section title={t("about.third_party")}>
        <ul className="credits">
          {THIRD_PARTY.map((c) => (
            <li key={c.name}>
              <Link href={c.url}>{c.name}</Link> <span className="hint">{c.license}</span>
            </li>
          ))}
        </ul>
      </Section>
    </>
  );
}
