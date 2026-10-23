import { useMemo } from 'react'
import { Button } from '@nyanpasu/ui/button'
import { ScrollArea } from '@nyanpasu/ui/scroll-area'
import { m } from '@/paraglide/messages'
import { isConfigItem, scopedTransformsOf, useProfile } from '@nyanpasu/query'
import { createFileRoute, useBlocker } from '@tanstack/react-router'
import PageToolbar from '../_modules/page-toolbar'
import { useTransformChainDraft } from '../_modules/use-transform-chain-draft'
import { isChainProfile } from '../_modules/utils'
import LeaveDialog from './_modules/leave-dialog'
import Library from './_modules/library'
import Panel from './_modules/panel'
import Pipeline from './_modules/pipeline'

export const Route = createFileRoute('/(main)/main/profiles/global')({
  component: RouteComponent,
  staticData: { layout: 'fill' },
})

const traceStepOf = (uid: string, index: number) => `global:${uid}:${index}`

function RouteComponent() {
  const { query, setGlobalTransforms } = useProfile()

  const transforms = useMemo(
    () => query.data?.global_transforms ?? [],
    [query.data?.global_transforms],
  )

  const chain = useTransformChainDraft({
    transforms,
    taskKey: 'update-global-transforms',
    onApply: (next) => setGlobalTransforms.mutateAsync(next),
  })

  const items = query.data?.items
  const currentUid = query.data?.current

  const active = useMemo(() => {
    const item = items?.find((profile) => profile.uid === currentUid)

    return item && isConfigItem(item) ? item : undefined
  }, [items, currentUid])

  const scoped = useMemo(() => {
    if (!active) return []

    const byUid = new Map(
      (items ?? []).filter(isChainProfile).map((item) => [item.uid, item]),
    )

    return scopedTransformsOf(active).flatMap((uid) => byUid.get(uid) ?? [])
  }, [active, items])

  const blocker = useBlocker({
    shouldBlockFn: () => true,
    disabled: !chain.isChanged,
    enableBeforeUnload: chain.isChanged,
    withResolver: true,
  })

  return (
    <section
      data-slot="profile-global-transforms"
      className="@container flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
    >
      <PageToolbar
        title={m.profile_global_transform_title()}
        backTo="/main/profiles"
        context={
          <p
            data-slot="profile-global-context"
            className="text-on-surface-variant truncate text-sm"
          >
            {active
              ? m.profile_global_context_active({ name: active.name })
              : m.profile_global_context_none()}
          </p>
        }
        actions={
          <>
            {chain.dataReady && chain.isChanged && (
              <span
                role="status"
                data-slot="profile-global-unapplied"
                className="bg-tertiary-container text-on-tertiary-container rounded-full px-3 py-1 text-xs font-medium"
              >
                {m.profile_global_unapplied()}
              </span>
            )}

            <Button
              onClick={chain.handleReset}
              disabled={!chain.isChanged || chain.actionsDisabled}
            >
              {m.common_reset()}
            </Button>

            <Button
              variant="flat"
              onClick={chain.handleApply}
              disabled={!chain.isChanged || chain.actionsDisabled}
            >
              {m.common_apply()}
            </Button>
          </>
        }
      />

      {/* One scroll region while the panels are stacked; from the wide layout
          on, each panel scrolls on its own and this area only clips. */}
      <ScrollArea
        data-slot="profile-global-body"
        className="min-h-0 flex-1 @[56rem]:[&>[data-slot=scroll-area-viewport]]:overflow-clip!"
      >
        <div className="grid gap-6 px-4 pb-4 md:px-6 @[56rem]:h-full @[56rem]:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] @[56rem]:grid-rows-[minmax(0,1fr)]">
          <Panel
            data-slot="profile-global-pipeline-panel"
            title={m.profile_global_pipeline_title()}
          >
            <Pipeline
              active={active}
              scoped={scoped}
              chain={chain}
              traceStepOf={traceStepOf}
            />
          </Panel>

          <Library chain={chain} />
        </div>
      </ScrollArea>

      <LeaveDialog
        open={blocker.status === 'blocked'}
        onStay={() => blocker.reset?.()}
        onLeave={() => blocker.proceed?.()}
      />
    </section>
  )
}
