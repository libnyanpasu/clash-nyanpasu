import { useLockFn } from '@nyanpasu/hooks'
import { isTauri } from '@nyanpasu/platform'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = isTauri() ? getCurrentWebviewWindow() : null

export type AsyncHandler<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
> = (...args: P) => Promise<void> | void

export type AsyncButtonOnClick<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
> = AsyncHandler<P>

export function useTrayClickHandler<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
>(onClick?: AsyncHandler<P>, disableClose?: boolean) {
  return useLockFn(async (...args: P) => {
    if (disableClose) {
      await onClick?.(...args)
      return
    }

    // Run the action before closing: closing decides, from the window's close
    // setting, whether the webview is destroyed, and a destroyed webview cannot
    // send IPC actions like quit_application. The menu stays visible while the
    // action runs; hiding it first would also make it lose focus, and the
    // backend closes the menu then.
    try {
      await onClick?.(...args)
    } finally {
      // A failed close must not hide the action's own error.
      await appWindow?.close().catch(console.error)
    }
  })
}
