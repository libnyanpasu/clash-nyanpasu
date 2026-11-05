import { rpc } from '@/services/rpc'
import { isTauri } from '@nyanpasu/platform'
import { type EditorWindowType } from '@nyanpasu/rpc/types'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

/** The route the editor window is served at; the desktop app opens the same one. */
function editorPath(windowType: EditorWindowType, uid: string | null) {
  switch (windowType) {
    case 'profile':
      return `/editor/profile?uid=${encodeURIComponent(uid ?? '')}`
    case 'css-editor':
      return '/editor/css'
  }
}

/**
 * Opens an editor in its own window: a native window on the desktop, a new
 * browser window (tab) elsewhere. The browser's window is same-origin, so it
 * shares the session and the query data the app loads.
 */
export async function openEditorWindow(
  windowType: EditorWindowType,
  uid: string | null,
) {
  if (isTauri()) {
    return rpc.createEditorWindow(windowType, uid)
  }

  window.open(editorPath(windowType, uid), '_blank')
}

/** Closes the window an editor runs in. A tab the user opened by hand stays. */
export async function closeEditorWindow() {
  if (isTauri()) {
    await getCurrentWebviewWindow().close()
  } else {
    window.close()
  }
}
