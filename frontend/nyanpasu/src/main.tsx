/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import React from 'react'
import { createRoot } from 'react-dom/client'
import { ResizeObserver } from '@juggle/resize-observer'
// Styles
import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import '@fontsource-variable/inter'
import { createRouter, RouterProvider } from '@tanstack/react-router'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import './assets/styles/index.css'
import './assets/styles/tailwind.css'
import { routeTree } from './route-tree.gen'
// installs error reporting before the app runs
import { reactRootErrorOptions } from './services/error-reporting'
// manually import language utils, inject paraglide custom strategy
import '@/utils/language'

if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver
}

// prepare dark mode class on root element before React hydration to avoid FOUC
document.documentElement.classList.toggle(
  'dark',
  window.matchMedia('(prefers-color-scheme: dark)').matches,
)

// Set up a Router instance
const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
})

// Register things for typesafety
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

const container = document.getElementById('root')!

createRoot(container, reactRootErrorOptions).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
)
