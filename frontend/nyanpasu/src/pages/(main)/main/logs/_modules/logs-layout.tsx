import { memo, type ReactNode } from 'react'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button'
import { m } from '@/paraglide/messages'
import { Route as IndexRoute, LogLevelsLayout } from '../route'

// The page re-renders for every incoming log; the tabs only change with the
// source.
const LogsSourceTabs = memo(function LogsSourceTabs({
  source,
}: {
  source: 'core' | 'app' | 'service'
}) {
  const navigate = IndexRoute.useNavigate()
  const labels = {
    core: m.logs_source_core(),
    app: m.logs_source_app(),
    service: m.logs_source_service(),
  }
  return (
    <SegmentedButton
      size="sm"
      className="w-auto shrink-0"
      aria-label={m.logs_source_label()}
      value={source}
      onValueChange={(item) => {
        // Selecting the selected segment again clears a toggle group.
        if (item === 'core' || item === 'app' || item === 'service')
          navigate({
            search: (previous) => ({ ...previous, source: item }),
          })
      }}
    >
      {(['core', 'app', 'service'] as const).map((item) => (
        <SegmentedButtonItem
          key={item}
          value={item}
          className="flex-none whitespace-nowrap"
        >
          {labels[item]}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
  )
})

export function LogsLayout({
  source,
  search,
  actions,
  children,
}: {
  source: 'core' | 'app' | 'service'
  search: ReactNode
  actions: ReactNode
  children: ReactNode
}) {
  return (
    <LogLevelsLayout>
      <div className="divide-outline-variant flex min-h-0 min-w-0 flex-1 flex-col divide-y">
        {children}
        <div
          className="bg-mixed-background flex min-h-16 shrink-0 flex-wrap items-center gap-3 px-4 py-3"
          data-slot="logs-toolbar"
        >
          <LogsSourceTabs source={source} />
          {search}
          {actions}
        </div>
      </div>
    </LogLevelsLayout>
  )
}
