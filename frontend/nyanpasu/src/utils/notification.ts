import { m } from '@/paraglide/messages'
import { commands, isIpcError, unwrapResult } from '@nyanpasu/interface'
import { isTauri } from '@tauri-apps/api/core'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import {
  MessageDialogOptions,
  message as tauriMessage,
} from '@tauri-apps/plugin-dialog'
import {
  isPermissionGranted,
  Options,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { formatError } from './index'
import { profileDialogLabel, profileMessageParts } from './profile-label'

let permissionGranted: boolean | null = null

const checkPermission = async () => {
  if (permissionGranted == null) {
    permissionGranted = await isPermissionGranted()
  }
  if (!permissionGranted) {
    const permission = await requestPermission()
    permissionGranted = permission === 'granted'
  }
  return permissionGranted
}

export type NotificationOptions = {
  title: string
  body?: string
  type?: NotificationType
}

export enum NotificationType {
  Success = 'success',
  Info = 'info',
  // Warn = "warn",
  Error = 'error',
}

export const notification = async ({
  title,
  body,
  type = NotificationType.Info,
}: NotificationOptions) => {
  if (!title) {
    throw new Error('missing message argument!')
  }
  if (!isTauri()) {
    window.alert(body ? `${title}: ${body}` : title)
    return
  }
  const permissionGranted = WIN_PORTABLE || (await checkPermission())
  if (WIN_PORTABLE || !permissionGranted) {
    await tauriMessage(body ? `${title}: ${body}` : title, {
      title: 'Clash Nyanpasu',
      kind: type === NotificationType.Error ? 'error' : 'info',
    })
    return
  }
  const options: Options = {
    title,
  }
  if (body) options.body = body
  sendNotification(options)
}

export type MessageOptions = MessageDialogOptions & {
  /** The caught error; a failed command adds a "copy error details" button. */
  error?: unknown
}

export const message = async (
  value: string,
  options?: string | MessageOptions | undefined,
) => {
  if (!isTauri()) {
    window.alert(value)
    return
  }
  if (typeof options === 'object') {
    const { error, ...dialog } = options
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
    const copyLabel = m.common_copy_error_details()
    const result = await tauriMessage(value, {
      ...dialog,
      ...(isIpcError(error) && {
        buttons: { ok: copyLabel, cancel: m.common_close() },
      }),
      title: dialog.title
        ? `Clash Nyanpasu - ${dialog.title}`
        : 'Clash Nyanpasu',
    })
    if (isIpcError(error) && result === copyLabel) {
      await writeText(error.detail)
    }
  } else {
    await tauriMessage(value, options)
  }
}
