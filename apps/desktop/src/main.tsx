import React from "react";
import ReactDOM from "react-dom/client";
// Bundle the design fonts locally (offline): IBM Plex Sans + IBM Plex Mono — an engineered,
// humanist pairing with real provenance (not the templated AI-tool defaults).
import "@fontsource-variable/ibm-plex-sans";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "@fontsource/ibm-plex-mono/600.css";
import { App } from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
