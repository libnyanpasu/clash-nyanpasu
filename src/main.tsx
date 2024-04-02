/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />

import { ResizeObserver } from "@juggle/resize-observer";
if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver;
}

import { Routes } from "@generouted/react-router/lazy";
import React from "react";
import { createRoot } from "react-dom/client";
import { RecoilRoot } from "recoil";
import "./services/i18n";
const container = document.getElementById("root")!;

createRoot(container).render(
  <React.StrictMode>
    <RecoilRoot>
      <Routes />
    </RecoilRoot>
  </React.StrictMode>,
);
