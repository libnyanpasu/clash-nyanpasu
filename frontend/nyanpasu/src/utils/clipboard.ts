import { commands } from '@/services/rpc'
import { isTauri } from '@nyanpasu/platform'
import { unwrapResult } from '@nyanpasu/rpc'

export const readClipboardText = async () => {
  if (!isTauri()) return navigator.clipboard.readText()
  return unwrapResult(await commands.readClipboardText())
}

export const writeClipboardText = async (text: string) => {
  if (!isTauri()) return navigator.clipboard.writeText(text)
  return unwrapResult(await commands.writeClipboardText(text))
}
