import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { notification, NotificationType } from '@/utils/notification'
import { listen, UnlistenFn } from '@tauri-apps/api/event'

export const NoticeProvider = () => {
  const { t } = useTranslation()
  const unlistenFn = useRef<UnlistenFn>(null)
  useEffect(() => {
    listen<{
      set_config: { ok: string } | { err: string }
    }>('nyanpasu://notice-message', ({ payload }) => {
      if ('ok' in payload?.set_config) {
        notification({
          title: t('Successful'),
          body: 'Refresh Clash Config',
          type: NotificationType.Success,
        })
      } else if ('err' in payload?.set_config) {
        notification({
          title: t('Error'),
          body: payload.set_config.err,
          type: NotificationType.Error,
        })
      }
    })
      .then((unlisten) => {
        unlistenFn.current = unlisten
      })
      .catch((e) => {
        notification({
          title: t('Error'),
          body: e.message,
          type: NotificationType.Error,
        })
      })
    return () => {
      unlistenFn.current?.()
    }
  }, [t])

  return null
}

export default NoticeProvider
