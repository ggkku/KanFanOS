import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

// 应用内不需要 WebView2 的默认右键菜单；所有页面统一屏蔽。
window.addEventListener(
  "contextmenu",
  (event) => event.preventDefault(),
  { capture: true },
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
