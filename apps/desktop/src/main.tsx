import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/inter/700.css";
import "@fontsource/inter/800.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/ui.css";
import "./styles/app.css";
import "./styles/widget.css";
import { Root } from "./Root";
import { currentSurface } from "./ipc/bridge";

const surface = currentSurface();
document.body.classList.add(`surface-${surface}`);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root surface={surface} />
  </React.StrictMode>,
);
