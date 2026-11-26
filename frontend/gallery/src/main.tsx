/// <reference types="vite/client" />
import React from 'react'
import { createRoot } from 'react-dom/client'
import '@csstools/normalize.css/normalize.css'
import '@csstools/normalize.css/opinionated.css'
import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import './styles/gallery.css'
import { App } from './app'

const container = document.getElementById('root')!

createRoot(container).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
