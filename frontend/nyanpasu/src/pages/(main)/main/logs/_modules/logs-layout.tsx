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

const standardSources = ['core', 'app', 'service'] as const
type LogTab = (typeof standardSources)[number] | 'logbook'

// The page re-renders for every incoming log; the tabs only change with the
// source.
const LogsSourceTabs = memo(function LogsSourceTabs({
  source,
  showLogbook,
}: {
  source: LogTab
  showLogbook: boolean
}) {
  const navigate = IndexRoute.useNavigate()
  const labels = {
    core: m.logs_source_core(),
    app: m.logs_source_app(),
    service: m.logs_source_service(),
    logbook: m.logs_source_logbook(),
  }
  const sources = showLogbook
    ? (['core', 'logbook', 'app', 'service'] as const)
    : standardSources
  return (
    <SegmentedButton
      variant="glass"
      size="sm"
      aria-label={m.logs_source_label()}
      value={source}
      onValueChange={(item) => {
        // Selecting the selected segment again clears a toggle group.
        const next = sources.find((source) => source === item)
        if (next)
          navigate({
            search: (previous) => ({ ...previous, source: next }),
          })
      }}
    >
      {sources.map((item) => (
        <SegmentedButtonItem key={item} value={item}>
          {labels[item]}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
  )
})

export function LogsLayout({
  source,
  showLogbook = false,
  search,
  actions,
  children,
}: {
  source: LogTab
  showLogbook?: boolean
  search: ReactNode
  actions: ReactNode
  children: ReactNode
}) {
  return (
    <LogLevelsLayout showLevels={source !== 'logbook'}>
      <DataPage
        title={m.navbar_label_logs()}
        toolbar={
          <DataPageToolbar data-slot="logs-toolbar">
            {search}
            <GlassControlGroup
              className="max-w-full overflow-x-auto"
              data-slot="data-page-controls"
            >
              <LogsSourceTabs source={source} showLogbook={showLogbook} />
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
    </LogLevelsLayout>
  )
}
