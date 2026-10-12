import { memo, type ReactNode } from 'react'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import {
  DataPage,
  dataPageFloatingGroupVariants,
  DataPageSurface,
  dataPageTabsVariants,
  DataPageToolbar,
} from '../../_modules/data-page'
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
      variant="tabs"
      size="sm"
      className={dataPageTabsVariants()}
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
          className="flex-none rounded-full whitespace-nowrap"
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
      <DataPage
        title={m.navbar_label_logs()}
        toolbar={
          <DataPageToolbar data-slot="logs-toolbar">
            {search}
            <div
              className={cn(
                dataPageFloatingGroupVariants(),
                'max-w-full overflow-x-auto',
              )}
              data-slot="data-page-controls"
            >
              <LogsSourceTabs source={source} />
            </div>
            <div
              className={cn(
                dataPageFloatingGroupVariants(),
                'max-w-full flex-wrap',
              )}
              data-slot="logs-actions"
            >
              {actions}
            </div>
          </DataPageToolbar>
        }
      >
        <DataPageSurface>{children}</DataPageSurface>
      </DataPage>
    </LogLevelsLayout>
  )
}
