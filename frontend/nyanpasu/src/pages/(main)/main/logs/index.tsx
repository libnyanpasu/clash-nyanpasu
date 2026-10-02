import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { useCoreStatus } from '@nyanpasu/query'
import { createFileRoute } from '@tanstack/react-router'
import FileLogs from './_modules/file-logs'
import { logPanelClass } from './_modules/log-viewer'
import { LogsLayout } from './_modules/logs-layout'
import { Route as IndexRoute } from './route'

export const Route = createFileRoute('/(main)/main/logs/')({
  component: RouteComponent,
})

function CoreLogs() {
  const core = useCoreStatus()
  const host = core.data?.type

  if (!host || core.isError) {
    return (
      <LogsLayout source="core" search={null} actions={null}>
        <div className={logPanelClass} data-slot="core-logs">
          <div
            role="status"
            className="text-on-surface-variant flex items-center gap-2 px-4 py-2 text-xs"
          >
            <span>
              {core.isPending ? m.logs_loading() : m.logs_source_unavailable()}
            </span>
            {!core.isPending && (
              <Button onClick={() => core.refetch()}>{m.logs_retry()}</Button>
            )}
          </div>
        </div>
      </LogsLayout>
    )
  }

  const source = host === 'service' ? 'core_service' : 'core_local'

  return <FileLogs key={source} source={source} displaySource="core" />
}

function RouteComponent() {
  const { source = 'core' } = IndexRoute.useSearch()

  return source === 'core' ? (
    <CoreLogs />
  ) : (
    <FileLogs key={source} source={source} />
  )
}
