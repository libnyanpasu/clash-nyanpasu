// Stands in for `@tauri-apps/api/webviewWindow`: the browser has no Tauri
// runtime, and some modules read the current window at import time.
export const getCurrentWebviewWindow = () => ({
  isMinimized: async () => false,
  close: async () => {},
  listen: async () => () => {},
  onResized: async () => () => {},
})

export class WebviewWindow {}
