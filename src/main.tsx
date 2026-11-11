import React from "react";
import ReactDOM from "react-dom/client";
import App from "@/App";
import { applyTheme } from "@/lib/theme";
// 视觉：先变量和两个本地字体，再 pk- 组件；点阵字 Doto 和说明用的 IBM Plex Sans 也打进包里（App 不联网取字体）。
import "@/vendor/peck/peck.css";
import "@/vendor/peck/bundle.css";
import "@fontsource/doto/900.css";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@/app.css";

// 先跟随系统；设置读出来以后（App 里）换成站主选的。
applyTheme("system");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
