import i18next, { type i18n } from "i18next";
import ICU from "i18next-icu";
import { initReactI18next } from "react-i18next";

import enCommon from "../../../../../assets/i18n/en/common.json";
import enNav from "../../../../../assets/i18n/en/nav.json";
import ruCommon from "../../../../../assets/i18n/ru/common.json";
import ruNav from "../../../../../assets/i18n/ru/nav.json";

export const namespaces = ["common", "nav"] as const;

export const resources = {
  ru: { common: ruCommon, nav: ruNav },
  en: { common: enCommon, nav: enNav },
} as const;

/** Creates and initialises an i18next instance (RU full, EN — empty slot falling back to RU). */
export async function createI18n(lng = "ru"): Promise<i18n> {
  const instance = i18next.createInstance();
  await instance
    .use(ICU)
    .use(initReactI18next)
    .init({
      lng,
      fallbackLng: "ru",
      ns: [...namespaces],
      defaultNS: "common",
      resources,
      returnEmptyString: false,
      interpolation: { escapeValue: false },
    });
  return instance;
}
