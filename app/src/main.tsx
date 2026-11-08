import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "./index.css";
import "./i18n/index.ts";
import { initToken, setToken } from "./api/client.ts";
import { App } from "./App.tsx";
import { installAudioUnlock } from "./state/audio.ts";
import { applyStoredTheme } from "./state/theme.ts";

setToken(initToken());
applyStoredTheme();
installAudioUnlock();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
