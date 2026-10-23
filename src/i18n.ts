import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { api, type Lang } from "./api";
import fr from "./locales/fr";
import en from "./locales/en";

void i18n.use(initReactI18next).init({
  resources: { fr: { translation: fr }, en: { translation: en } },
  lng: "fr",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

/** Follows the language resolved by the backend, including live changes. */
export function syncLanguage() {
  const apply = (lang: Lang) => {
    void i18n.changeLanguage(lang);
    document.documentElement.lang = lang;
  };
  api.lang().then(apply, () => undefined);
  void listen<Lang>("lang", (e) => apply(e.payload));
}

export default i18n;
