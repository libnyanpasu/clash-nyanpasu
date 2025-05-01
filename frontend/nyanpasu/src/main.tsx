/// <reference types="vite/client" />
/// <reference types="vite-plugin-svgr/client" />
import React from 'react'
import { createRoot } from 'react-dom/client'
import { ResizeObserver } from '@juggle/resize-observer'
// Styles
import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import { createRouter, RouterProvider } from '@tanstack/react-router'
import './assets/styles/index.scss'
import './assets/styles/tailwind.css'
import { routeTree } from './routeTree.gen'
import './services/i18n'

if (!window.ResizeObserver) {
  window.ResizeObserver = ResizeObserver
}

window.addEventListener('error', (event) => {
  console.error(event)
})

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

createRoot(container).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
)
