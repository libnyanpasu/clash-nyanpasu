import WarningRounded from '~icons/material-symbols/warning-rounded'
import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { AnimatedItem } from '@/components/ui/animated-item'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { CircularProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { move } from '@dnd-kit/helpers'
import { DragDropProvider, useDroppable } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import {
  scopedTransformsOf,
  useProfile,
  type ProfileDefinition_Deserialize,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import {
  isChainProfile,
  type ConfigProfile,
  type TransformProfile,
} from '../../_modules/utils'

type ScriptOrMergeProfile = TransformProfile

enum ColumnType {
  Active = 'active',
  Inactive = 'inactive',
}

const COLUMN_TYPES = [ColumnType.Active, ColumnType.Inactive] as const

// CollisionPriority.Low in @dnd-kit/abstract 0.5.0, which neither @dnd-kit/react
// nor @dnd-kit/dom re-exports. A column must lose to the items inside it,
// otherwise a pointer resting on an item but nearer the column centre drops at
// the column edge instead.
const COLUMN_COLLISION_PRIORITY = 1

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

const ItemButton = ({
  profile,
  buttonRef,
  disabled,
}: {
  profile: ScriptOrMergeProfile
  buttonRef?: (element: Element | null) => void
  disabled?: boolean
}) => (
  <Button
    className="bg-secondary-container/30 h-14 w-full rounded-2xl text-left"
    variant="raised"
    disabled={disabled}
    asChild
  >
    <button ref={buttonRef} disabled={disabled}>
      <TextMarquee className="pointer-events-none">{profile.name}</TextMarquee>
    </button>
  </Button>
)

const Item = ({
  profile,
  index,
  column,
  saving,
}: {
  profile: ScriptOrMergeProfile
  index: number
  column: ColumnType
  saving: boolean
}) => {
  const { ref } = useSortable({
    id: profile.uid,
    index,
    // The group names the list this item belongs to: move() resolves the target
    // column by looking the group up as a key of the draft record.
    group: column,
    type: 'item',
    accept: ['item'],
    disabled: saving,
  })

  return <ItemButton profile={profile} buttonRef={ref} disabled={saving} />
}

const Column = ({
  profiles,
  type,
  saving,
  readOnly,
}: {
  profiles: ScriptOrMergeProfile[]
  type: ColumnType
  saving: boolean
  readOnly: boolean
}) => {
  const { ref } = useDroppable({
    id: type,
    type: 'column',
    accept: ['item'],
    collisionPriority: COLUMN_COLLISION_PRIORITY,
    disabled: saving || readOnly,
  })

  const message = {
    [ColumnType.Active]: m.profile_chain_editor_active_column(),
    [ColumnType.Inactive]: m.profile_chain_editor_inactive_column(),
  }

  return (
    <Card variant="outline" asChild>
      <div ref={ref}>
        <CardHeader>{message[type]}</CardHeader>

        <CardContent>
          {profiles.map((profile, index) =>
            readOnly ? (
              // Duplicate uids would otherwise register two sortables under one
              // id, so a read-only column renders plain buttons.
              <ItemButton
                key={`${profile.uid}-${index}`}
                profile={profile}
                disabled
              />
            ) : (
              <Item
                key={profile.uid}
                profile={profile}
                index={index}
                column={type}
                saving={saving}
              />
            ),
          )}
        </CardContent>
      </div>
    </Card>
  )
}

export default function ChianEditorCard({
  profile,
}: {
  profile: ConfigProfile
}) {
  // Keyed per profile so a draft cannot leak into another config whose baseline
  // happens to be identical.
  return <ChainEditorSession key={profile.uid} profile={profile} />
}

function ChainEditorSession({ profile }: { profile: ConfigProfile }) {
  const {
    query: { data: profiles },
    replaceDefinition,
  } = useProfile()

  const dataReady = profiles != null

  // Candidate chain items = every Transform profile (Overlay / Script).
  const candidateProfiles = useMemo<ScriptOrMergeProfile[]>(
    () => (profiles?.items ?? []).filter(isChainProfile),
    [profiles?.items],
  )

  const candidateUids = useMemo(
    () => candidateProfiles.map((item) => item.uid),
    [candidateProfiles],
  )

  // The edited config item's own scoped transforms (File or Composition).
  const currentTransforms = useMemo(
    () => scopedTransformsOf(profile),
    [profile],
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
    const active = draft[ColumnType.Active]
      .map((uid) => renderedProfiles.find((item) => item.uid === uid))
      .filter((item): item is ScriptOrMergeProfile => Boolean(item))

    const inactive = draft[ColumnType.Inactive]
      .map((uid) => renderedProfiles.find((item) => item.uid === uid))
      .filter((item): item is ScriptOrMergeProfile => Boolean(item))

    return {
      [ColumnType.Active]: active,
      [ColumnType.Inactive]: inactive,
    }
  }, [draft, renderedProfiles])

  // Only the active column is persisted, so reordering the inactive one is not
  // a pending change.
  const isChanged = !sameOrder(
    draft[ColumnType.Active],
    serverChains[ColumnType.Active],
  )

  const blockTask = useBlockTask(
    `update-chain-${profile.uid}`,
    async (transforms: string[]) => {
      try {
        // Rebuild the config definition with the reordered scoped transforms,
        // preserving every other field, and replace it atomically.
        const definition: ProfileDefinition_Deserialize = {
          type: 'config',
          config: { ...profile.config, transforms },
        }

        await replaceDefinition.mutateAsync({ uid: profile.uid, definition })
      } catch (error) {
        message(`Update failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        })
      }
    },
  )

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

  return (
    <Card className="relative col-span-2 md:col-span-4">
      <AnimatePresence initial={false}>
        {saving && (
          <motion.div
            data-slot="core-manager-card-mask"
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

      <CardContent className="grid sm:grid-cols-2">
        <DragDropProvider
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
          <Column
            profiles={chains.active}
            type={ColumnType.Active}
            saving={saving}
            readOnly={!editable && !dragSession}
          />

          <Column
            profiles={chains.inactive}
            type={ColumnType.Inactive}
            saving={saving}
            readOnly={!editable && !dragSession}
          />
        </DragDropProvider>
      </CardContent>

      <AnimatePresence>
        {dataReady && isChanged && (
          <AnimatedItem>
            <CardFooter className="gap-1">
              <Button
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
