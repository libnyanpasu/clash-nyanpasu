import { useLockFn } from '@/hooks/use-lock-fn'
import { useSetting } from '@nyanpasu/interface'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

const appWindow = getCurrentWebviewWindow()

export type AsyncHandler<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
> = (...args: P) => Promise<void> | void

export type AsyncButtonOnClick<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
> = AsyncHandler<P>

export function useTrayClickHandler<
  P extends unknown[] = [React.MouseEvent<HTMLButtonElement>],
>(onClick?: AsyncHandler<P>, disableClose?: boolean) {
  const { value: closeBehavior } = useSetting('tray_menu_close_behavior')

  return useLockFn(async (...args: P) => {
    if (disableClose) {
      await onClick?.(...args)
      return
    }

    if (closeBehavior === 'close') {
      // Run the action before closing. Closing first can destroy this webview
      // before IPC actions like quit_application are sent.
      try {
        await onClick?.(...args)
      } finally {
        await appWindow.close()
      }
    } else {
      // Hide mode (default): hide immediately for fast visual response.
      await appWindow.hide()
      await onClick?.(...args)
    }
  })
}
