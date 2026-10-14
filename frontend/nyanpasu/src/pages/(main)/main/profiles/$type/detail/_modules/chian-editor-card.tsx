import WarningRounded from '~icons/material-symbols/warning-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { AnimatedItem } from '@nyanpasu/ui/animated-item'
import { Button } from '@nyanpasu/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@nyanpasu/ui/card'
import { CircularProgress } from '@nyanpasu/ui/progress'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider, KeyboardSensor, PointerSensor } from '@dnd-kit/react'
import {
  scopedTransformsOf,
  useProfile,
  useProfileMutations,
} from '@nyanpasu/query'
import { type ProfileDefinition_Deserialize } from '@nyanpasu/rpc/types'
import { cn } from '@nyanpasu/utils'
import {
  categoryProfiles,
  isChainProfile,
  type ConfigProfile,
  type TransformProfile,
} from '../../_modules/utils'
import {
  TransformChainColumn,
  type TransformChainLayout,
} from './transform-chain-column'

type ScriptOrMergeProfile = TransformProfile

// The whole card is the drag source, so the default mouse activation would
// require a press-and-hold; touch keeps its delay to stay scrollable.
const SENSORS = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType === 'mouse') return undefined

      const defaults = PointerSensor.defaults.activationConstraints

      return typeof defaults === 'function' ? defaults(event, source) : defaults
    },
  }),
  KeyboardSensor,
]

enum ColumnType {
  Active = 'active',
  Inactive = 'inactive',
}

const COLUMN_TYPES = [ColumnType.Active, ColumnType.Inactive] as const

type ChainColumns = Record<ColumnType, string[]>

type DragSession = {
  columns: ChainColumns
  profiles: ScriptOrMergeProfile[]
}

const sameOrder = (left: string[], right: string[]) =>
  left.length === right.length &&
  left.every((uid, index) => uid === right[index])

const sameColumns = (left: ChainColumns, right: ChainColumns) =>
  COLUMN_TYPES.every((type) => sameOrder(left[type], right[type]))

/** Server order wins; uids without a matching candidate are dropped. */
const deriveColumns = (
  transforms: string[],
  candidateUids: string[],
): ChainColumns => {
  const known = new Set(candidateUids)
  const active = transforms.filter((uid) => known.has(uid))
  const selected = new Set(active)

  return {
    [ColumnType.Active]: active,
    [ColumnType.Inactive]: candidateUids.filter((uid) => !selected.has(uid)),
  }
}

/** Keeps the draft order while candidates are added or removed. */
const reconcileCandidates = (
  draft: ChainColumns,
  candidateUids: string[],
): ChainColumns => {
  const known = new Set(candidateUids)
  const active = draft[ColumnType.Active].filter((uid) => known.has(uid))
  const selected = new Set(active)
  const inactive = draft[ColumnType.Inactive].filter(
    (uid) => known.has(uid) && !selected.has(uid),
  )
  const present = new Set([...active, ...inactive])

  return {
    [ColumnType.Active]: active,
    [ColumnType.Inactive]: [
      ...inactive,
      ...candidateUids.filter((uid) => !present.has(uid)),
    ],
  }
}

export default function ChianEditorCard({
  profile,
}: {
  profile: ConfigProfile
}) {
  const { replaceDefinition } = useProfileMutations()

  // The edited config item's own scoped transforms (File or Composition).
  const transforms = useMemo(() => scopedTransformsOf(profile), [profile])

  // Keyed per profile so a draft cannot leak into another config whose baseline
  // happens to be identical.
  return (
    <TransformChainEditor
      key={profile.uid}
      layout="stacked"
      className="col-span-2 md:col-span-4"
      taskKey={`update-chain-${profile.uid}`}
      transforms={transforms}
      onApply={async (next) => {
        // Rebuild the config definition with the reordered scoped transforms,
        // preserving every other field, and replace it atomically.
        const definition: ProfileDefinition_Deserialize = {
          type: 'config',
          config: { ...profile.config, transforms: next },
        }

        await replaceDefinition.mutateAsync({ uid: profile.uid, definition })
      }}
    />
  )
}

/** Shared drag-and-drop editor for scoped and global transforms. */
export function TransformChainEditor({
  transforms: currentTransforms,
  taskKey,
  onApply,
  className,
  layout,
}: {
  /** The persisted chain, in order. */
  transforms: string[]
  taskKey: string
  onApply: (transforms: string[]) => Promise<unknown>
  className?: string
  layout: TransformChainLayout
}) {
  const {
    query: { data: profiles },
  } = useProfile()

  const dataReady = profiles != null

  // Candidate chain items = every Transform profile (Overlay / Script).
  const candidateProfiles = useMemo<ScriptOrMergeProfile[]>(() => {
    const candidates = (profiles?.items ?? []).filter(isChainProfile)
    if (layout === 'stacked') return candidates

    return Object.values(categoryProfiles(candidates))
      .flat()
      .filter(isChainProfile)
  }, [profiles?.items, layout])

  const candidateUids = useMemo(
    () => candidateProfiles.map((item) => item.uid),
    [candidateProfiles],
  )

  const serverChains = useMemo(
    () => deriveColumns(currentTransforms, candidateUids),
    [currentTransforms, candidateUids],
  )

  const [draft, setDraft] = useState<ChainColumns>(serverChains)

  // The draft is checked alongside the config because a corrected server
  // snapshot lands one commit before the sync effect replaces the draft, and
  // two sortables sharing a uid clobber each other in the dnd-kit registry.
  const hasDuplicates =
    new Set(currentTransforms).size !== currentTransforms.length ||
    new Set(draft[ColumnType.Active]).size !== draft[ColumnType.Active].length

  const draftRef = useRef(draft)
  // null until the first server snapshot is adopted, so the initial load wins.
  const adoptedActiveRef = useRef<string[] | null>(null)
  const dragSessionRef = useRef<DragSession | null>(null)
  const [dragSession, setDragSession] = useState<DragSession | null>(null)
  const savingRef = useRef(false)

  const publishDraft = useCallback((next: ChainColumns) => {
    draftRef.current = next
    setDraft(next)
  }, [])

  useEffect(() => {
    if (!dataReady || dragSessionRef.current) return

    const serverActive = serverChains[ColumnType.Active]
    const serverChanged =
      adoptedActiveRef.current === null ||
      !sameOrder(adoptedActiveRef.current, serverActive)

    // A changed chain is the server's call; a changed candidate list only adds
    // or removes entries and must not discard the draft order.
    const next = serverChanged
      ? serverChains
      : reconcileCandidates(draftRef.current, candidateUids)

    adoptedActiveRef.current = serverActive

    if (!sameColumns(next, draftRef.current)) {
      publishDraft(next)
    }
  }, [serverChains, candidateUids, dragSession, dataReady, publishDraft])

  // Snapshot the candidates for the duration of a gesture so a refetch cannot
  // unmount the element being dragged.
  const renderedProfiles = dragSession?.profiles ?? candidateProfiles

  const chains = useMemo(() => {
    const byUid = new Map(renderedProfiles.map((item) => [item.uid, item]))
    const resolve = (uids: string[]) =>
      uids
        .map((uid) => byUid.get(uid))
        .filter((item): item is ScriptOrMergeProfile => Boolean(item))

    return {
      [ColumnType.Active]: resolve(draft[ColumnType.Active]),
      [ColumnType.Inactive]: resolve(draft[ColumnType.Inactive]),
    }
  }, [draft, renderedProfiles])

  // Only the active column is persisted, so reordering the inactive one is not
  // a pending change.
  const isChanged = !sameOrder(
    draft[ColumnType.Active],
    serverChains[ColumnType.Active],
  )

  const blockTask = useBlockTask(taskKey, async (transforms: string[]) => {
    try {
      await onApply(transforms)
    } catch (error) {
      message(`Update failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    }
  })

  const saving = blockTask.isPending
  const editable = dataReady && !hasDuplicates
  const actionsDisabled = !editable || saving || dragSession !== null

  const handleApply = async () => {
    if (!editable || savingRef.current || dragSessionRef.current) return

    savingRef.current = true

    try {
      const next = reconcileCandidates(draftRef.current, candidateUids)
      if (!sameColumns(next, draftRef.current)) publishDraft(next)

      await blockTask.execute(next[ColumnType.Active])
    } finally {
      savingRef.current = false
    }
  }

  const handleReset = () => {
    if (!editable || savingRef.current || dragSessionRef.current) return

    adoptedActiveRef.current = serverChains[ColumnType.Active]
    publishDraft(serverChains)
  }

  const loadingMessage = m.profile_chain_editor_apply_message()

  // Stable so the memoized columns do not re-render on every drag-over.
  const handleToggle = useCallback(
    (uid: string) => {
      if (!editable || savingRef.current || dragSessionRef.current) return

      const current = draftRef.current
      const source = current.active.includes(uid)
        ? ColumnType.Active
        : ColumnType.Inactive
      const target =
        source === ColumnType.Active ? ColumnType.Inactive : ColumnType.Active

      publishDraft({
        ...current,
        [source]: current[source].filter((item) => item !== uid),
        [target]: [...current[target], uid],
      })
    },
    [editable, publishDraft],
  )

  return (
    <Card
      data-slot="profile-chain-editor"
      data-layout={layout}
      className={cn('relative', className)}
    >
      {layout === 'stacked' && (
        <CardHeader className="flex-col items-start gap-1">
          <h2 className="text-lg font-medium">
            {m.profile_chain_editor_title()}
          </h2>
          <p className="text-on-surface-variant text-sm">
            {m.profile_chain_editor_scoped_hint()}
          </p>
        </CardHeader>
      )}
      <AnimatePresence initial={false}>
        {saving && (
          <motion.div
            data-slot="profile-chain-editor-saving-mask"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className={cn(
              'bg-primary/10 absolute inset-0 z-50 backdrop-blur-3xl',
              'flex flex-col items-center justify-center gap-4',
            )}
          >
            <CircularProgress className="size-12" indeterminate />

            <p>{loadingMessage}</p>
          </motion.div>
        )}
      </AnimatePresence>

      {hasDuplicates && (
        <CardContent className="pb-0">
          <p role="alert" className="text-error flex items-start gap-2 text-sm">
            <WarningRounded className="mt-0.5 size-4 shrink-0" />
            {m.profile_chain_editor_duplicate_hint()}
          </p>
        </CardContent>
      )}

      <CardContent
        data-slot="profile-chain-editor-columns"
        className={cn(
          'grid',
          layout === 'grouped' && 'md:grid-cols-[1.25fr_1fr]',
        )}
      >
        <DragDropProvider
          sensors={SENSORS}
          onBeforeDragStart={(event) => {
            if (!editable || savingRef.current) event.preventDefault()
          }}
          onDragStart={(_event, manager) => {
            // dnd-kit awaits a render between beforedragstart and dragstart and
            // its continuation re-checks neither the source nor the editor, so
            // a save or a correction can have landed in that window.
            if (!editable || savingRef.current) {
              manager.actions.stop({ canceled: true })
              return
            }

            const session = {
              columns: draftRef.current,
              profiles: candidateProfiles,
            }

            dragSessionRef.current = session
            setDragSession(session)
          }}
          onDragOver={(event) => {
            // Synchronous and unconditional: the optimistic sorting plugin only
            // reads defaultPrevented in a microtask, so this reliably stops it
            // from relocating React-owned nodes into the other column.
            event.preventDefault()

            if (!dragSessionRef.current || savingRef.current) return

            const next = move(draftRef.current, event)
            if (!sameColumns(next, draftRef.current)) publishDraft(next)
          }}
          onDragEnd={(event) => {
            const session = dragSessionRef.current
            if (event.canceled && session) publishDraft(session.columns)

            dragSessionRef.current = null
            setDragSession(null)
          }}
        >
          <TransformChainColumn
            profiles={chains.active}
            type={ColumnType.Active}
            saving={saving}
            readOnly={!editable && !dragSession}
            layout={layout}
            onToggle={handleToggle}
            actionsDisabled={actionsDisabled}
          />

          <TransformChainColumn
            profiles={chains.inactive}
            type={ColumnType.Inactive}
            saving={saving}
            readOnly={!editable && !dragSession}
            layout={layout}
            onToggle={handleToggle}
            actionsDisabled={actionsDisabled}
          />
        </DragDropProvider>
      </CardContent>

      <p
        data-slot="profile-chain-editor-drag-hint"
        className="text-on-surface-variant px-4 pb-4 text-xs"
      >
        {m.profile_chain_editor_drag_hint()}
      </p>

      <AnimatePresence>
        {dataReady && isChanged && (
          <AnimatedItem>
            <CardFooter className="gap-1">
              <Button
                variant="flat"
                className="flex items-center gap-2"
                onClick={handleApply}
                disabled={actionsDisabled}
              >
                {m.common_apply()}
              </Button>

              <Button onClick={handleReset} disabled={actionsDisabled}>
                {m.common_reset()}
              </Button>
            </CardFooter>
          </AnimatedItem>
        )}
      </AnimatePresence>
    </Card>
  )
}
