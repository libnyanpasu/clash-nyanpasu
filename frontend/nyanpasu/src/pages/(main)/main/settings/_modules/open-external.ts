import { commands } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { createBrowserAdapter, detectRuntime } from '@nyanpasu/platform'

/** Opens a link in the system browser rather than in the webview. */
export async function openExternal(url: string) {
  try {
    if (detectRuntime().bridge === 'tauri') await commands.openThat(url)
    else createBrowserAdapter()?.openExternal(url)
  } catch (error) {
    message(formatError(error), { kind: 'error', error })
  }
}
