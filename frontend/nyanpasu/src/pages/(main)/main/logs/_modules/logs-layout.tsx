import AppsRounded from '~icons/material-symbols/apps-rounded'
import CheckRounded from '~icons/material-symbols/check-rounded'
import DnsRounded from '~icons/material-symbols/dns-rounded'
import MemoryRounded from '~icons/material-symbols/memory-rounded'
import type { ReactNode } from 'react'
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs'
import { m } from '@/paraglide/messages'
import { Route as IndexRoute, LogLevelsLayout } from '../route'

export function LogsLayout({
  source,
  actions,
  children,
}: {
  source: 'core' | 'app' | 'service'
  actions: ReactNode
  children: ReactNode
}) {
  const navigate = IndexRoute.useNavigate()
  const icons = { core: MemoryRounded, app: AppsRounded, service: DnsRounded }
  const labels = {
    core: m.logs_source_core(),
    app: m.logs_source_app(),
    service: m.logs_source_service(),
  }
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div
        className="flex shrink-0 flex-wrap items-center gap-3 p-3"
        role="group"
        aria-label={m.logs_source_label()}
      >
        <AnimatedTabs
          variant="segment"
          activeTab={source}
          className="w-full shrink-0 sm:w-80"
          onChange={(item) => {
            if (item === 'core' || item === 'app' || item === 'service')
              navigate({
                search: (previous) => ({ ...previous, source: item }),
              })
          }}
        >
          {(['core', 'app', 'service'] as const).map((item) => {
            const Icon = source === item ? CheckRounded : icons[item]
            return (
              <AnimatedTabsItem
                key={item}
                value={item}
                id={`logs-source-${item}`}
                aria-controls="logs-source-panel"
              >
                <Icon aria-hidden className="hidden size-5 shrink-0 sm:block" />
                <span>{labels[item]}</span>
              </AnimatedTabsItem>
            )
          })}
        </AnimatedTabs>
        <div className="ml-auto flex min-w-0 flex-1 items-center justify-end gap-2">
          {actions}
        </div>
      </div>
      <div
        id="logs-source-panel"
        role="tabpanel"
        aria-labelledby={`logs-source-${source}`}
        className="flex min-h-0 min-w-0 flex-1 flex-col"
      >
        <LogLevelsLayout>{children}</LogLevelsLayout>
      </div>
    </div>
  )
}
