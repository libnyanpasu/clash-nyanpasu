import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useMemo, useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { AnimatedItem } from '@/components/ui/animated-item'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardFooter, CardHeader } from '@/components/ui/card'
import { CircularProgress } from '@/components/ui/progress'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
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

const CHAIN_EDITOR_SORTABLE_GROUP = 'chain-editor-sortable'

const Item = ({
  profile,
  index,
}: {
  profile: ScriptOrMergeProfile
  index: number
}) => {
  const { ref } = useSortable({
    id: profile.uid,
    index,
    group: CHAIN_EDITOR_SORTABLE_GROUP,
    type: 'item',
    accept: ['item'],
  })

  return (
    <Button
      className="bg-secondary-container/30 h-14 w-full rounded-2xl text-left"
      variant="raised"
      asChild
    >
      <button ref={ref}>
        <TextMarquee className="pointer-events-none">
          {profile.name}
        </TextMarquee>
      </button>
    </Button>
  )
}

const Column = ({
  profiles,
  type,
}: {
  profiles: ScriptOrMergeProfile[]
  type: ColumnType
}) => {
  const { ref } = useDroppable({
    id: type,
    type: 'column',
    accept: ['item'],
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
          {profiles.map((profile, index) => (
            <Item key={profile.uid} profile={profile} index={index} />
          ))}
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
  const {
    query: { data: profiles },
    replaceDefinition,
  } = useProfile()

  // Candidate chain items = every Transform profile (Overlay / Script).
  const scriptProfiles = useMemo<ScriptOrMergeProfile[]>(
    () => (profiles?.items ?? []).filter(isChainProfile),
    [profiles?.items],
  )

  const scriptProfileUids = useMemo(() => {
    return scriptProfiles.map((item) => item.uid)
  }, [scriptProfiles])

  // The edited config item's own scoped transforms (File or Composition).
  const currentTransforms = useMemo(
    () => scopedTransformsOf(profile),
    [profile],
  )

  const [chainsUids, setChainsUids] = useState<Record<ColumnType, string[]>>({
    [ColumnType.Active]: [],
    [ColumnType.Inactive]: [],
  })

  const hasSameOrder = (left: string[], right: string[]) => {
    if (left.length !== right.length) {
      return false
    }

    return left.every((item, index) => item === right[index])
  }

  const setChainsUidsIfChanged = (
    updater: (
      prev: Record<ColumnType, string[]>,
    ) => Record<ColumnType, string[]>,
  ) => {
    setChainsUids((prev) => {
      const next = updater(prev)
      const unchanged = COLUMN_TYPES.every((type) =>
        hasSameOrder(prev[type], next[type]),
      )

      return unchanged ? prev : next
    })
  }

  const chains = useMemo(() => {
    const active = chainsUids[ColumnType.Active]
      .map((uid) => scriptProfiles.find((item) => item.uid === uid))
      .filter((item): item is ScriptOrMergeProfile => Boolean(item))

    const inactive = chainsUids[ColumnType.Inactive]
      .map((uid) => scriptProfiles.find((item) => item.uid === uid))
      .filter((item): item is ScriptOrMergeProfile => Boolean(item))

    return {
      [ColumnType.Active]: active,
      [ColumnType.Inactive]: inactive,
    }
  }, [chainsUids, scriptProfiles])

  // sync chains with the config item's scoped transforms and scriptProfiles
  useEffect(() => {
    const activeSet = new Set(currentTransforms)
    const nextActive = scriptProfileUids.filter((uid) => activeSet.has(uid))
    const nextInactive = scriptProfileUids.filter((uid) => !activeSet.has(uid))

    setChainsUids((prev) => {
      const activeUnchanged = hasSameOrder(prev[ColumnType.Active], nextActive)

      const inactiveUnchanged = hasSameOrder(
        prev[ColumnType.Inactive],
        nextInactive,
      )

      if (activeUnchanged && inactiveUnchanged) {
        return prev
      }

      return {
        [ColumnType.Active]: nextActive,
        [ColumnType.Inactive]: nextInactive,
      }
    })
  }, [scriptProfileUids, currentTransforms])

  const isChanged = useMemo(() => {
    const activeSet = new Set(currentTransforms)
    const baselineActive = scriptProfileUids.filter((uid) => activeSet.has(uid))
    const baselineInactive = scriptProfileUids.filter(
      (uid) => !activeSet.has(uid),
    )

    return (
      !hasSameOrder(chainsUids[ColumnType.Active], baselineActive) ||
      !hasSameOrder(chainsUids[ColumnType.Inactive], baselineInactive)
    )
  }, [chainsUids, currentTransforms, scriptProfileUids])

  const blockTask = useBlockTask(`update-chain-${profile.uid}`, async () => {
    try {
      const nextTransforms = chainsUids[ColumnType.Active]
      // Rebuild the config definition with the reordered scoped transforms,
      // preserving every other field, and replace it atomically.
      const definition: ProfileDefinition_Deserialize = {
        type: 'config',
        config: { ...profile.config, transforms: nextTransforms },
      }

      await replaceDefinition.mutateAsync({ uid: profile.uid, definition })
    } catch (error) {
      message(`Update failed: \n ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      })
    }
  })

  const handleApply = useLockFn(blockTask.execute)

  const loadingMessage = m.profile_chain_editor_apply_message()

  return (
    <Card className="relative col-span-2 md:col-span-4">
      <AnimatePresence initial={false}>
        {blockTask.isPending && (
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

      <CardContent className="grid sm:grid-cols-2">
        <DragDropProvider
          onDragEnd={(event) => {
            setChainsUidsIfChanged((prev) => move(prev, event))
          }}
        >
          <Column profiles={chains.active} type={ColumnType.Active} />

          <Column profiles={chains.inactive} type={ColumnType.Inactive} />
        </DragDropProvider>
      </CardContent>

      <AnimatePresence>
        {isChanged && (
          <AnimatedItem>
            <CardFooter className="gap-1">
              <Button className="flex items-center gap-2" onClick={handleApply}>
                {m.common_apply()}
              </Button>
            </CardFooter>
          </AnimatedItem>
        )}
      </AnimatePresence>
    </Card>
  )
}
