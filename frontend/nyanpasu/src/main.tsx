/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import React from "react";
import { createRoot } from "react-dom/client";
import { Routes } from "@generouted/react-router/lazy";
import { ResizeObserver } from "@juggle/resize-observer";
// Styles
import "./assets/styles/index.scss";
import "./assets/styles/tailwind.css";
import "@csstools/normalize.css/normalize.css";
import "@csstools/normalize.css/opinionated.css";
import "./services/i18n";

if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver;
}

const container = document.getElementById("root")!;

createRoot(container).render(
  <React.StrictMode>
    <Routes />
  </React.StrictMode>,
);
