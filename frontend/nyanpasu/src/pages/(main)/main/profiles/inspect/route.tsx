import { useEffect, useState } from 'react'
import { z } from 'zod'
import { useOpenProfile, useProfileLookup } from '@/components/profile-label'
import { keepReturn } from '@/components/router/cross-navigation'
import { m } from '@/paraglide/messages'
import { events, queries } from '@/services/rpc'
import { unwrapQueryOptions, useKvStorage } from '@nyanpasu/query'
import {
  useQuery,
  useQueryClient,
  type QueryClient,
} from '@tanstack/react-query'
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router'
import {
  SnapshotStatus,
  SnapshotSwitch,
  type SnapshotView,
} from './_modules/inspect-toolbar'
import {
  DEFAULT_INSPECT_VIEW,
  INSPECT_VIEW_KV_KEY,
  toInspectView,
} from './_modules/inspect-view'
import InspectViewer from './_modules/inspect-viewer'

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
  validateSearch: z.object({ step: z.string().optional() }),
  staticData: { layout: 'fill' },
})

function invalidateSnapshots(queryClient: QueryClient) {
  for (const query of [
    queries.inspectRuntime(),
    queries.inspectAppliedRuntime(),
  ]) {
    queryClient.invalidateQueries({ queryKey: query.queryKey })
  }
}

function loadNode(snapshotId: string, nodeId: number) {
  const query = queries.inspectRuntimeNode(snapshotId, nodeId)
  return unwrapQueryOptions(query, query.queryFn!)
}

function RouteComponent() {
  const queryClient = useQueryClient()
  const navigate = useNavigate({ from: Route.fullPath })
  const { step } = Route.useSearch()
  const profiles = useProfileLookup()
  const openProfile = useOpenProfile(profiles)
  const [view, setView] = useState<SnapshotView>('latest')
  const [inspectView, setInspectView] = useKvStorage(
    INSPECT_VIEW_KV_KEY,
    DEFAULT_INSPECT_VIEW,
    { migrate: toInspectView },
  )

  const inspectionQuery =
    view === 'applied'
      ? queries.inspectAppliedRuntime()
      : queries.inspectRuntime()
  const inspection = useQuery({
    ...unwrapQueryOptions(inspectionQuery, inspectionQuery.queryFn!),
    refetchOnWindowFocus: false,
    retry: false,
  })

  // Every rebuild or apply publishes a configuration status change, so the
  // snapshots are refetched on it instead of by hand.
  useEffect(() => {
    const listener = events.configurationStatusChanged
      .listen(() => invalidateSnapshots(queryClient))
      .catch(() => undefined)
    return () => {
      listener.then((unlisten) => unlisten?.())
    }
  }, [queryClient])

  return (
    <InspectViewer
      source={{ summary: inspection, loadNode }}
      selectedStep={step}
      onStepChange={(step) =>
        navigate({
          to: '.',
          search: (previous) => ({ ...previous, step }),
          replace: true,
          state: keepReturn,
        })
      }
      inspectView={inspectView}
      onViewChange={(update) => {
        // A view that fails to save still applies; it is only not remembered.
        setInspectView((previous) => ({ ...previous, ...update }))
      }}
      profiles={profiles}
      onOpenProfile={openProfile}
      globalLink={
        <Link to="/main/profiles/global">{m.inspect_edit_in_global()}</Link>
      }
      snapshotControls={<SnapshotSwitch view={view} onViewChange={setView} />}
      status={inspection.data && <SnapshotStatus snapshot={inspection.data} />}
      onRefresh={() => invalidateSnapshots(queryClient)}
    />
  )
}
