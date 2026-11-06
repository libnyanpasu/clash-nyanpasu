import { useState } from 'react'
import { z } from 'zod'
import { DEFAULT_INSPECT_VIEW } from '@/pages/(main)/main/profiles/inspect/_modules/inspect-view'
import InspectViewer from '@/pages/(main)/main/profiles/inspect/_modules/inspect-viewer'
import { m } from '@/paraglide/messages'
import { queries } from '@/services/rpc'
import { unwrapQueryOptions } from '@nyanpasu/query'
import { isIpcError } from '@nyanpasu/rpc'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(inspector)/rejected-config/')({
  component: RejectedConfigPage,
  validateSearch: z.object({ id: z.string() }),
})

function snapshotUnavailable(error: unknown) {
  return (
    isIpcError(error) &&
    error.kind.domain === 'runtime' &&
    error.kind.error.kind === 'runtime_snapshot_changed'
  )
}

function RejectedConfigPage() {
  const { id } = Route.useSearch()
  const [step, setStep] = useState<string>()
  const [view, setView] = useState(DEFAULT_INSPECT_VIEW)
  const [nodeUnavailable, setNodeUnavailable] = useState(false)
  const query = queries.inspectRejectedRuntime(id)
  const inspection = useQuery({
    ...unwrapQueryOptions(query, query.queryFn!),
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: 0,
  })
  const unavailable = nodeUnavailable || snapshotUnavailable(inspection.error)

  const loadNode = (snapshotId: string, nodeId: number) => {
    const query = queries.inspectRejectedNode(snapshotId, nodeId)
    const options = unwrapQueryOptions(query, query.queryFn!)
    return {
      ...options,
      queryFn: async () => {
        try {
          return await options.queryFn()
        } catch (error) {
          if (snapshotUnavailable(error)) setNodeUnavailable(true)
          throw error
        }
      },
    }
  }

  return (
    <InspectViewer
      title={m.inspect_rejected_title()}
      titleHidden
      emptyMessage={m.inspect_rejected_replaced()}
      source={{
        summary: {
          data: unavailable ? null : inspection.data,
          isPending: inspection.isPending,
          isError: !unavailable && inspection.isError,
          error: inspection.error,
        },
        loadNode,
      }}
      selectedStep={step}
      onStepChange={setStep}
      inspectView={view}
      onViewChange={(update) =>
        setView((previous) => ({ ...previous, ...update }))
      }
    />
  )
}
