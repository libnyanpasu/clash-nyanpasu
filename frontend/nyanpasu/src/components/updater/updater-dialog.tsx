import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { useSetAtom } from 'jotai'
import { lazy, Suspense, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UpdaterIgnoredAtom } from '@/store/updater'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Button, LinearProgress } from '@mui/material'
import { cleanupProcesses, openThat } from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps, cn } from '@nyanpasu/ui'
import { relaunch } from '@tauri-apps/plugin-process'
import { DownloadEvent, type Update } from '@tauri-apps/plugin-updater'
import styles from './updater-dialog.module.scss'

const Markdown = lazy(() => import('react-markdown'))

export interface UpdaterDialogProps extends Omit<BaseDialogProps, 'title'> {
  update: Update
}

export default function UpdaterDialog({
  open,
  update,
  onClose,
  ...others
}: UpdaterDialogProps) {
  const { t } = useTranslation()
  const setUpdaterIgnore = useSetAtom(UpdaterIgnoredAtom)
  const [contentLength, setContentLength] = useState(0)
  const [contentDownloaded, setContentDownloaded] = useState(0)
  const progress =
    contentDownloaded && contentLength
      ? (contentDownloaded / contentLength) * 100
      : 0
  const onDownloadEvent = useCallback((e: DownloadEvent) => {
    switch (e.event) {
      case 'Started':
        setContentLength(e.data.contentLength || 0)
        break
      case 'Progress':
        setContentDownloaded((prev) => prev + e.data.chunkLength)
        break
    }
  }, [])

  const handleUpdate = useLockFn(async () => {
    try {
      // Install the update. This will also restart the app on Windows!
      await update.download(onDownloadEvent)
      await cleanupProcesses()
      // cleanup and stop core
      await update.install()
      // On macOS and Linux you will need to restart the app manually.
      // You could use this step to display another confirmation dialog.
      await relaunch()
    } catch (e) {
      console.error(e)
      message(formatError(e), { kind: 'error', title: t('Error') })
    }
  })

  return (
    <BaseDialog
      {...others}
      title={t('updater.title')}
      open={open}
      onClose={() => {
        setUpdaterIgnore(update.version) // TODO: control this behavior
        onClose?.()
      }}
      onOk={handleUpdate}
      close={t('updater.close')}
      ok={t('updater.update')}
      divider
    >
      <div
        className={cn(
          'xs:min-w-[90vw] sm:min-w-[50vw] md:min-w-[33.3vw]',
          styles.UpdaterDialog,
        )}
      >
        <div className="flex items-center justify-between px-2 py-2">
          <div className="flex gap-3">
            <span className="text-xl font-bold">{update.version}</span>
            <span className="text-xs text-slate-500">
              {dayjs(update.date, 'YYYY-MM-DD H:mm:ss Z').format(
                'YYYY-MM-DD HH:mm:ss',
              )}
            </span>
          </div>
          <Button
            variant="contained"
            size="small"
            onClick={() => {
              openThat(
                `https://github.com/libnyanpasu/clash-nyanpasu/releases/tag/v${update.version}`,
              )
            }}
          >
            {t('updater.go')}
          </Button>
        </div>
        <div
          className={cn('h-[50vh] overflow-y-auto p-4', styles.MarkdownContent)}
        >
          <Suspense fallback={<div>{t('loading')}</div>}>
            <Markdown
              components={{
                a(props) {
                  const { children, node, ...rest } = props
                  return (
                    <a
                      {...rest}
                      onClick={(e) => {
                        e.preventDefault()
                        e.stopPropagation()
                        if (typeof node?.properties.href === 'string') {
                          openThat(node.properties.href)
                        }
                      }}
                    >
                      {children}
                    </a>
                  )
                },
              }}
            >
              {update.body || 'New version available.'}
            </Markdown>
          </Suspense>
        </div>
        {contentLength && (
          <div className="mt-2 flex items-center gap-2">
            <LinearProgress
              className="flex-1"
              variant="determinate"
              value={progress}
            />
            <span className="text-xs text-slate-500">
              {progress.toFixed(2)}%
            </span>
          </div>
        )}
      </div>
    </BaseDialog>
  )
}
