/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import "./assets/styles/index.scss";
import "./assets/styles/tailwind.css";

import { ResizeObserver } from "@juggle/resize-observer";
if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver;
}

import React from "react";
import { createRoot } from "react-dom/client";
import { Routes } from "@generouted/react-router/lazy";
import "./services/i18n";
const container = document.getElementById("root")!;

createRoot(container).render(
  <React.StrictMode>
    <Routes />
  </React.StrictMode>,
);
