import { memo, type ReactNode } from 'react'
import { GlassControlGroup } from '@nyanpasu/ui/glass-control-group'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import {
  DataPage,
  DataPageSurface,
  DataPageToolbar,
} from '../../_modules/data-page'
import { Route as IndexRoute, LogLevelsLayout } from '../route'

type LogsSource = 'core' | 'app' | 'service' | 'audit'

// The page re-renders for every incoming log; the tabs only change with the
// source.
const LogsSourceTabs = memo(function LogsSourceTabs({
  source,
}: {
  source: LogsSource
}) {
  const navigate = IndexRoute.useNavigate()
  const labels = {
    core: m.logs_source_core(),
    app: m.logs_source_app(),
    service: m.logs_source_service(),
    audit: m.logs_source_audit(),
  }
  return (
    <SegmentedButton
      variant="glass"
      size="sm"
      aria-label={m.logs_source_label()}
      value={source}
      onValueChange={(item) => {
        // Selecting the selected segment again clears a toggle group.
        if (item === 'audit') navigate({ to: '/main/logs/audit' })
        else if (item === 'core' || item === 'app' || item === 'service')
          navigate({
            to: '/main/logs',
            search: (previous) => ({ level: previous.level, source: item }),
          })
      }}
    >
      {(['core', 'app', 'service', 'audit'] as const).map((item) => (
        <SegmentedButtonItem key={item} value={item}>
          {labels[item]}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
  )
})

/** The logs data page and its toolbar, without a sidebar. */
export function LogsDataPage({
  source,
  search,
  actions,
  children,
}: {
  source: LogsSource
  search?: ReactNode
  actions: ReactNode
  children: ReactNode
}) {
  return (
    <DataPage
      title={m.navbar_label_logs()}
      toolbar={
        <DataPageToolbar data-slot="logs-toolbar">
          {search}
          <GlassControlGroup
            className="max-w-full overflow-x-auto"
            data-slot="data-page-controls"
          >
            <LogsSourceTabs source={source} />
          </GlassControlGroup>
          <GlassControlGroup
            className="max-w-full flex-wrap"
            data-slot="logs-actions"
          >
            {actions}
          </GlassControlGroup>
        </DataPageToolbar>
      }
    >
      <DataPageSurface>{children}</DataPageSurface>
    </DataPage>
  )
}

export function LogsLayout({
  source,
  ...props
}: {
  source: 'core' | 'app' | 'service'
  search: ReactNode
  actions: ReactNode
  children: ReactNode
}) {
  return (
    <LogLevelsLayout>
      <LogsDataPage source={source} {...props} />
    </LogLevelsLayout>
  )
}
