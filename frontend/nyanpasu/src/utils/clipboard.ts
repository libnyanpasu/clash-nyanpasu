import { commands } from '@/services/rpc'
import { detectRuntime } from '@nyanpasu/platform'

export const readClipboardText = async () => {
  const runtime = detectRuntime()
  if (runtime.bridge === 'none' && runtime.platform.surface === 'browser')
    return navigator.clipboard.readText()
  if (runtime.bridge !== 'tauri')
    throw new Error('Clipboard is unavailable in this runtime')
  return await commands.readClipboardText()
}

export const writeClipboardText = async (text: string) => {
  const runtime = detectRuntime()
  if (runtime.bridge === 'none' && runtime.platform.surface === 'browser')
    return navigator.clipboard.writeText(text)
  if (runtime.bridge !== 'tauri')
    throw new Error('Clipboard is unavailable in this runtime')
  return await commands.writeClipboardText(text)
}
