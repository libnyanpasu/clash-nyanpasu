import { commands } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { isTauri } from '@nyanpasu/platform'

/** Opens a link in the system browser rather than in the webview. */
export async function openExternal(url: string) {
  try {
    if (isTauri()) await commands.openThat(url)
    else window.open(url, '_blank', 'noopener,noreferrer')
  } catch (error) {
    message(formatError(error), { kind: 'error', error })
  }
}
