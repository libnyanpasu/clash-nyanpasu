import { useState } from 'react'
import { z } from 'zod'
import { useProfileLookup } from '@/components/profile-label'
import { DEFAULT_INSPECT_VIEW } from '@/pages/(main)/main/profiles/inspect/_modules/inspect-view'
import InspectViewer from '@/pages/(main)/main/profiles/inspect/_modules/inspect-viewer'
import { m } from '@/paraglide/messages'
import { commands, queries } from '@/services/rpc'
import { isIpcError, rpcDomainError } from '@nyanpasu/rpc'
import { useQuery } from '@tanstack/react-query'
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/(inspector)/candidate-config/')({
  component: CandidateConfigPage,
  validateSearch: z.object({ id: z.string() }),
})

function snapshotUnavailable(error: unknown) {
  const domain = rpcDomainError(error)
  return (
    isIpcError(domain) &&
    domain.kind.domain === 'runtime' &&
    domain.kind.error.kind === 'runtime_snapshot_changed'
  )
}

function CandidateConfigPage() {
  const { id } = Route.useSearch()
  // The rows and a stopped step name profiles the way the inspector does.
  const profiles = useProfileLookup()
  const [step, setStep] = useState<string>()
  const [view, setView] = useState(DEFAULT_INSPECT_VIEW)
  const [nodeUnavailable, setNodeUnavailable] = useState(false)
  const inspection = useQuery({
    ...queries.inspectCandidateRuntime(id),
    retry: false,
    refetchOnWindowFocus: false,
    gcTime: 0,
  })

  const unavailable = nodeUnavailable || snapshotUnavailable(inspection.error)

  const loadNode = (snapshotId: string, nodeId: number) => {
    return {
      queryKey: queries.inspectCandidateNode(snapshotId, nodeId).queryKey,
      queryFn: async () => {
        try {
          return await commands.inspectCandidateNode(snapshotId, nodeId)
        } catch (error) {
          if (snapshotUnavailable(error)) setNodeUnavailable(true)
          throw error
        }
      },
    }
  }

  const candidate = unavailable ? undefined : inspection.data

  return (
    <InspectViewer
      title={m.inspect_candidate_title()}
      titleHidden
      emptyMessage={m.inspect_candidate_unavailable()}
      source={{
        summary: {
          data: unavailable ? null : inspection.data?.inspection,
          isPending: inspection.isPending,
          isError: !unavailable && inspection.isError,
          error: inspection.error,
        },
        loadNode,
      }}
      candidate={candidate}
      profiles={profiles}
      selectedStep={step}
      onStepChange={setStep}
      inspectView={view}
      onViewChange={(update) =>
        setView((previous) => ({ ...previous, ...update }))
      }
    />
  )
}
