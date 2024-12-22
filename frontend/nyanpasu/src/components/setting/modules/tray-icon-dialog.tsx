import { useMemoizedFn } from 'ahooks'
import { useState, useTransition } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'
import { formatError, sleep } from '@/utils'
import { message } from '@/utils/notification'
import { LoadingButton } from '@mui/lab'
import {
  getServerPort,
  isTrayIconSet,
  setTrayIcon as setTrayIconCall,
} from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps } from '@nyanpasu/ui'
import { open } from '@tauri-apps/plugin-dialog'

function TrayIconItem({ mode }: { mode: 'system_proxy' | 'tun' | 'normal' }) {
  const { t } = useTranslation()
  const [ts, setTs] = useState(Date.now())
  const {
    data: isSetTrayIcon,
    isLoading,
    mutate,
  } = useSWR('/isSetTrayIcon?mode=' + mode, () => isTrayIconSet(mode), {
    revalidateOnFocus: true,
  })
  const { data: serverPort } = useSWR('/getServerPort', getServerPort)
  const src = `http://localhost:${serverPort}/tray/icon?mode=${mode}&ts=${ts}`
  const [loading, startTransition] = useTransition()
  const selectImage = async () => {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [
        { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'ico'] },
      ],
    })
    if (Array.isArray(selected)) {
      throw new Error('Not Support')
    } else if (selected === null) {
      return null
    } else {
      return selected
    }
  }

  const setTrayIcon = useMemoizedFn((reset?: boolean) => {
    startTransition(async () => {
      try {
        const selected = reset ? undefined : await selectImage()
        if (selected === null) {
          return
        }
        return await setTrayIconCall(mode, selected)
      } catch (e) {
        message(formatError(e), {
          kind: 'error',
        })
      } finally {
        setTs(Date.now())
        await sleep(2000)
        await mutate()
      }
    })
  })

  return (
    <div className="flex items-center justify-between">
      <div className="flex items-center gap-3">
        <img className="h-14 w-14" src={src} draggable={false} loading="lazy" />
        <span className="text-base font-semibold">{t(mode)}</span>
      </div>
      <span>
        {isSetTrayIcon ? (
          <div className="flex gap-3">
            <LoadingButton
              variant="contained"
              loading={isLoading || loading}
              disabled={loading || isLoading}
              onClick={() => setTrayIcon()}
            >
              {t('Edit')}
            </LoadingButton>
            <LoadingButton
              variant="contained"
              loading={isLoading || loading}
              disabled={loading || isLoading}
              onClick={() => setTrayIcon(true)}
            >
              {t('Reset')}
            </LoadingButton>
          </div>
        ) : (
          <LoadingButton
            variant="contained"
            loading={isLoading || loading}
            disabled={loading || isLoading}
            onClick={() => setTrayIcon()}
          >
            {t('Set')}
          </LoadingButton>
        )}
      </span>
    </div>
  )
}

export type TrayIconDialogProps = Omit<BaseDialogProps, 'title'>

export default function TrayIconDialog({
  open,
  onClose,
  ...props
}: TrayIconDialogProps) {
  const { t } = useTranslation()
  return (
    <BaseDialog
      title={t('Tray Icons')}
      open={open}
      onClose={onClose}
      {...props}
    >
      <div className="grid gap-3">
        <TrayIconItem mode="normal" />
        <TrayIconItem mode="tun" />
        <TrayIconItem mode="system_proxy" />
      </div>
    </BaseDialog>
  )
}
