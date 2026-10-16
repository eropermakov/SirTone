import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import { App } from "./app/App";
import { createI18n } from "./shared/i18n";
import "./index.css";

const root = document.getElementById("root");
if (root) {
  const i18n = await createI18n("ru");
  createRoot(root).render(
    <StrictMode>
      <I18nextProvider i18n={i18n}>
        <App />
      </I18nextProvider>
    </StrictMode>,
  );
}
