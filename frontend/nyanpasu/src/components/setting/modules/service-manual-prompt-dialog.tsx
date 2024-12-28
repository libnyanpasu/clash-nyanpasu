import { useAsyncEffect } from 'ahooks'
import { useAtom, useSetAtom } from 'jotai'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'
import { OS } from '@/consts'
import { serviceManualPromptDialogAtom } from '@/store/service'
import { getShikiSingleton } from '@/utils/shiki'
import { useTheme } from '@mui/material'
import { getCoreDir, getServiceInstallPrompt } from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps, cn } from '@nyanpasu/ui'
import './service-manual-prompt-dialog.scss'

export type ServerManualPromptDialogProps = Omit<BaseDialogProps, 'title'> & {
  operation: 'uninstall' | 'install' | 'start' | 'stop' | null
}

// TODO: maybe support more commands prompt?
export default function ServerManualPromptDialog({
  open,
  onClose,
  operation,
  ...props
}: ServerManualPromptDialogProps) {
  const { t } = useTranslation()
  const theme = useTheme()
  const { data: serviceInstallPrompt, error } = useSWR(
    operation === 'install' ? '/service_install_prompt' : null,
    getServiceInstallPrompt,
  )
  const { data: coreDir } = useSWR('/core_dir', () => getCoreDir())
  const [codes, setCodes] = useState<string | null>(null)
  useAsyncEffect(async () => {
    if (operation === 'install' && serviceInstallPrompt) {
      const shiki = await getShikiSingleton()
      const code = await shiki.codeToHtml(
        `cd "${coreDir}"\n${serviceInstallPrompt}`,
        {
          lang: 'shell',
          themes: {
            dark: 'nord',
            light: 'min-light',
          },
        },
      )
      setCodes(code)
    } else if (operation) {
      const shiki = await getShikiSingleton()
      const code = await shiki.codeToHtml(
        `cd "${coreDir}"\n${OS !== 'windows' ? 'sudo ' : ''}./nyanpasu-service ${operation}`,
        {
          lang: 'shell',
          themes: {
            dark: 'nord',
            light: 'min-light',
          },
        },
      )
      setCodes(code)
    }
  }, [serviceInstallPrompt, operation, coreDir])

  return (
    <BaseDialog
      title={t('Service Manual Tips')}
      open={open}
      onClose={onClose}
      {...props}
    >
      <div className="grid gap-3">
        <p>
          {t('Unable to operation the service automatically', {
            operation: t(`${operation}`),
          })}
        </p>
        {error && <p className="text-red-500">{error.message}</p>}
        {!!codes && (
          <div
            className={cn(
              'max-w-[80vw] rounded-sm',
              theme.palette.mode === 'dark' && 'dark',
            )}
            dangerouslySetInnerHTML={{
              __html: codes,
            }}
          />
        )}
      </div>
    </BaseDialog>
  )
}

export function ServerManualPromptDialogWrapper() {
  const [prompt, setPrompt] = useAtom(serviceManualPromptDialogAtom)
  return (
    <ServerManualPromptDialog
      open={!!prompt}
      onClose={() => setPrompt(null)}
      operation={prompt}
    />
  )
}

export function useServerManualPromptDialog() {
  const setPrompt = useSetAtom(serviceManualPromptDialogAtom)
  return {
    show: (prompt: 'install' | 'uninstall' | 'stop' | 'start') =>
      setPrompt(prompt),
    close: () => setPrompt(null),
  }
}
