import { lazy, RefObject, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { LogProvider } from '@/components/logs/log-provider'
import LogHeader from '@/components/logs/los-header'
import { BasePage } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(legacy)/logs')({
  component: LogPage,
})

function LogPage() {
  const { t } = useTranslation()

  const viewportRef = useRef<HTMLDivElement>(null)

  const Component = lazy(() => import('@/components/logs/log-page'))

  return (
    <LogProvider>
      <BasePage
        full
        title={t('Logs')}
        header={<LogHeader />}
        viewportRef={viewportRef}
      >
        <Component scrollRef={viewportRef as RefObject<HTMLElement>} />
      </BasePage>
    </LogProvider>
  )
}
