/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import React from 'react'
import { createRoot } from 'react-dom/client'
import { ResizeObserver } from '@juggle/resize-observer'
import { RouterProvider } from './router'
// Styles
import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import './assets/styles/index.scss'
import './assets/styles/tailwind.css'
import './services/i18n'
// manually import language utils, inject paraglide custom strategy
import '@/utils/language'

if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver
}

window.addEventListener('error', (event) => {
  console.error(event)
})

// prepare dark mode class on root element before React hydration to avoid FOUC
document.documentElement.classList.toggle(
  'dark',
  window.matchMedia('(prefers-color-scheme: dark)').matches,
)

const container = document.getElementById('root')!

createRoot(container).render(
  <React.StrictMode>
    <RouterProvider />
  </React.StrictMode>,
)
