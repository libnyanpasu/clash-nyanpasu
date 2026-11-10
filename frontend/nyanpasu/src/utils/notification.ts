import { m } from '@/paraglide/messages'
import { commands } from '@/services/rpc'
import { writeClipboardText } from '@/utils/clipboard'
import {
  createBrowserAdapter,
  detectRuntime,
  showWebNotification,
} from '@nyanpasu/platform'
import { isIpcError, unwrapResult } from '@nyanpasu/rpc'
import type {
  IpcError,
  NativeDialogButtons,
  NativeDialogKind,
} from '@nyanpasu/rpc/types'
import { formatError } from './index'
import { profileDialogLabel, profileMessageParts } from './profile-label'

export type NotificationOptions = {
  title: string
  body?: string
  type?: NotificationType
}

export enum NotificationType {
  Success = 'success',
  Info = 'info',
  Error = 'error',
}

export const notification = async ({ title, body }: NotificationOptions) => {
  if (!title) {
    throw new Error('missing message argument!')
  }
  const runtime = detectRuntime()
  if (runtime.platform.surface === 'browser') {
    await showWebNotification({ title, body })
    return
  }
  if (
    runtime.platform.surface === 'desktop' &&
    runtime.platform.clientOs === 'windows' &&
    unwrapResult(await commands.isPortable())
  ) {
    await message(body ? `${title}: ${body}` : title, {
      title: 'Clash Nyanpasu',
    })
    return
  }

  if (runtime.bridge !== 'tauri')
    throw new Error('Notifications are unavailable in this runtime')
  unwrapResult(await commands.showNativeNotification(title, body ?? null))
}

type MessageButtons =
  | 'Ok'
  | 'OkCancel'
  | 'YesNo'
  | 'YesNoCancel'
  | { ok?: string; cancel?: string; yes?: string; no?: string }

export type MessageOptions = {
  title?: string
  kind?: NativeDialogKind
  buttons?: MessageButtons
  okLabel?: string
  /** The caught error; a failed command adds a copy-details button. */
  error?: unknown
}

const nativeButtons = (
  buttons: MessageButtons | undefined,
  okLabel: string | undefined,
  cancelLabel: string,
): NativeDialogButtons => {
  if (typeof buttons === 'string') {
    const types = {
      Ok: 'ok',
      OkCancel: 'ok_cancel',
      YesNo: 'yes_no',
      YesNoCancel: 'yes_no_cancel',
    } as const
    return { type: types[buttons] }
  }
  if (buttons?.yes && buttons.no) {
    return {
      type: 'yes_no_cancel_custom',
      yes: buttons.yes,
      no: buttons.no,
      cancel: buttons.cancel ?? cancelLabel,
    }
  }
  if (buttons?.ok && buttons.cancel) {
    return {
      type: 'ok_cancel_custom',
      ok: buttons.ok,
      cancel: buttons.cancel,
    }
  }
  if (buttons?.ok ?? okLabel) {
    return { type: 'ok_custom', ok: buttons?.ok ?? okLabel! }
  }
  if (buttons?.cancel) {
    return {
      type: 'ok_cancel_custom',
      ok: 'Ok',
      cancel: buttons.cancel,
    }
  }
  return { type: buttons?.yes ? 'yes_no' : 'ok' }
}

export const ask = async (
  value: string,
  options?: string | Pick<MessageOptions, 'title' | 'kind'>,
) => {
  const runtime = detectRuntime()
  if (runtime.platform.surface === 'browser') {
    return createBrowserAdapter()?.confirm(value) ?? false
  }
  if (runtime.bridge !== 'tauri')
    throw new Error('Dialogs are unavailable in this runtime')

  const title = typeof options === 'string' ? options : (options?.title ?? null)
  const kind = typeof options === 'string' ? 'info' : (options?.kind ?? 'info')
  return unwrapResult(await commands.askNativeDialog(value, title, kind))
}

export const message = async (
  value: string,
  options?: string | MessageOptions | undefined,
) => {
  const dialog = typeof options === 'object' ? options : undefined
  const error = dialog?.error
  if (isIpcError(error)) {
    const parts = profileMessageParts((label) => formatError(error, label))
    if (parts.some((part) => typeof part !== 'string')) {
      try {
        const profiles = unwrapResult(await commands.getProfiles())
        const lookup = new Map(
          profiles.items.map((profile) => [profile.uid, profile]),
        )
        value = value.replace(formatError(error), () =>
          formatError(error, (id) => profileDialogLabel(lookup, id)),
        )
      } catch {
        // A failed name lookup must not hide the original command failure.
      }
    }
  }

  const runtime = detectRuntime()
  if (runtime.platform.surface === 'browser') {
    const browser = createBrowserAdapter()
    browser?.alert(value)
    if (
      isIpcError(error) &&
      browser?.confirm(`${m.common_copy_error_details()}?`)
    ) {
      await writeClipboardText(error.detail)
    }
    return
  }
  if (runtime.bridge !== 'tauri')
    throw new Error('Dialogs are unavailable in this runtime')

  const copyLabel = m.common_copy_error_details()
  const buttons = nativeButtons(
    dialog?.buttons,
    dialog?.okLabel,
    m.common_close(),
  )
  if (isIpcError(error)) {
    return showErrorMessage(value, dialog, copyLabel, error)
  }

  const title =
    typeof options === 'string'
      ? options
      : dialog?.title
        ? `Clash Nyanpasu - ${dialog.title}`
        : 'Clash Nyanpasu'
  unwrapResult(
    await commands.showNativeMessageDialog(
      value,
      title,
      dialog?.kind ?? 'info',
      buttons,
    ),
  )
}

const showErrorMessage = async (
  value: string,
  dialog: MessageOptions | undefined,
  copyLabel: string,
  error: IpcError,
) => {
  const title = dialog?.title
    ? `Clash Nyanpasu - ${dialog.title}`
    : 'Clash Nyanpasu'
  const buttons: NativeDialogButtons = {
    type: 'ok_cancel_custom',
    ok: copyLabel,
    cancel: m.common_close(),
  }
  const result = unwrapResult(
    await commands.showNativeMessageDialog(
      value,
      title,
      dialog?.kind ?? 'info',
      buttons,
    ),
  )
  if (result === copyLabel) await writeClipboardText(error.detail)
}
