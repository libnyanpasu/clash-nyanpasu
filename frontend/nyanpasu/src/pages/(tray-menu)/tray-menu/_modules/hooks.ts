import { useLockFn } from '@/hooks/use-lock-fn'
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
  return useLockFn(async (...args: P) => {
    if (!disableClose) {
      await appWindow.hide()
    }

    try {
      await onClick?.(...args)
    } finally {
      if (!disableClose) {
        await appWindow.close()
      }
    }
  })
}
