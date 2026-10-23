import CheckRounded from '~icons/material-symbols/check-rounded'
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded'
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useMemo } from 'react'
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

  // Which configs run each transform, for the library rows.
  const usedBy = useMemo(() => {
    const names = new Map<string, string[]>()

    for (const item of items ?? []) {
      if (!isConfigItem(item)) continue

      for (const uid of new Set(scopedTransformsOf(item))) {
        names.set(uid, [...(names.get(uid) ?? []), item.name])
      }
    }

    return names
  }, [items])

  const usedByOf = useCallback((uid: string) => usedBy.get(uid) ?? [], [usedBy])

  const blocker = useBlocker({
    shouldBlockFn: () => true,
    disabled: !chain.isChanged,
    enableBeforeUnload: chain.isChanged,
    withResolver: true,
  })

  return (
    <section
      data-slot="profile-global-transforms"
      className="@container relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
    >
      <PageToolbar
        title={m.profile_global_transform_title()}
        backTo="/main/profiles"
        context={
          <p
            data-slot="profile-global-context"
            className="text-on-surface-variant flex h-8 min-w-0 items-center gap-1.5 rounded-lg px-3 text-[13px] shadow-[inset_0_0_0_1px_var(--color-outline-variant)] max-md:hidden"
          >
            <DescriptionOutlineRounded
              className="size-4 shrink-0"
              aria-hidden
            />

            {active ? (
              <span className="truncate">
                {m.profile_global_applies_to()}{' '}
                <strong className="text-on-surface font-semibold">
                  {active.name}
                </strong>
              </span>
            ) : (
              <span className="truncate">
                {m.profile_global_context_none()}
              </span>
            )}
          </p>
        }
        actions={
          <>
            {chain.dataReady && chain.isChanged && (
              <span
                role="status"
                data-slot="profile-global-unapplied"
                className="bg-tertiary-container text-on-tertiary-container flex h-7 items-center gap-1.5 rounded-full px-2.5 text-xs font-semibold whitespace-nowrap before:size-1.5 before:rounded-full before:bg-current"
              >
                {m.profile_global_unapplied()}
              </span>
            )}
          </>
        }
      />

      {/* One scroll region while the columns are stacked; from the wide layout
          on, each column scrolls on its own and this area only clips. */}
      <ScrollArea
        data-slot="profile-global-body"
        className="min-h-0 flex-1 @[56rem]:[&>[data-slot=scroll-area-viewport]]:overflow-clip!"
      >
        <div className="flex flex-col @[56rem]:h-full @[56rem]:flex-row">
          <Pipeline
            active={active}
            scoped={scoped}
            chain={chain}
            traceStepOf={traceStepOf}
          />

          <Library chain={chain} usedByOf={usedByOf} />
        </div>
      </ScrollArea>

      <AnimatePresence>
        {chain.dataReady && chain.isChanged && (
          <motion.div
            data-slot="profile-global-actions"
            className="absolute right-4 bottom-4 z-10 flex items-center gap-2"
            initial={{ opacity: 0, y: 16 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 16 }}
          >
            <Button
              variant="fab"
              className="flex items-center gap-2"
              onClick={chain.handleReset}
              disabled={chain.actionsDisabled}
            >
              <RestartAltRounded className="size-6" />

              {m.common_reset()}
            </Button>

            <Button
              variant="fab"
              className="bg-primary text-on-primary dark:bg-primary dark:text-on-primary flex items-center gap-2"
              onClick={chain.handleApply}
              disabled={chain.actionsDisabled}
            >
              <CheckRounded className="size-6" />

              {m.common_apply()}
            </Button>
          </motion.div>
        )}
      </AnimatePresence>

      <LeaveDialog
        open={blocker.status === 'blocked'}
        onStay={() => blocker.reset?.()}
        onLeave={() => blocker.proceed?.()}
      />
    </section>
  )
}
