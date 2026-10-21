import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import { App } from "./app/App";
import { createI18n } from "./shared/i18n";
import { getSettings } from "./shared/ipc/settings";
import "./index.css";

const root = document.getElementById("root");
if (root) {
  // The language of the settings (T6.5); Russian when unknown.
  const lang = await getSettings()
    .then((s) => (s.language === "en" ? "en" : "ru"))
    .catch(() => "ru");
  const i18n = await createI18n(lang);
  createRoot(root).render(
    <StrictMode>
      <I18nextProvider i18n={i18n}>
        <App />
      </I18nextProvider>
    </StrictMode>,
  );
}
