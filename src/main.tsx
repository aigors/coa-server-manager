import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./i18n";
import "./index.css";

if (import.meta.env.VITE_COA_WEB === "1") {
  // Web build: the backend is coa-web over HTTP (services/coa-web).
  await import("./web/bridge");
} else if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  await import("./dev/mock");
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>,
);
