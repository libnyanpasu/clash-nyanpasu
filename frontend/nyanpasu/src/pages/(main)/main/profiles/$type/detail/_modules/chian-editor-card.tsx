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
import { move } from '@dnd-kit/helpers'
import { DragDropProvider, useDroppable } from '@dnd-kit/react'
import { useSortable } from '@dnd-kit/react/sortable'
import {
  LocalProfile,
  ProfileBuilder,
  RemoteProfile,
  useProfile,
} from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'
import { categoryProfiles, CategoryProfiles } from '../../_modules/utils'
import { ProfileType } from '../../../_modules/consts'

type ScriptOrMergeProfile = CategoryProfiles[
  | ProfileType.JavaScript
  | ProfileType.Lua
  | ProfileType.Merge][number]

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
  profile: LocalProfile | RemoteProfile
}) {
  const {
    query: { data: profiles },
    patch,
  } = useProfile()

  const categorizedProfiles = useMemo(() => {
    if (!profiles?.items) {
      return null
    }

    return categoryProfiles(profiles.items)
  }, [profiles?.items])

  const scriptProfiles = useMemo<ScriptOrMergeProfile[]>(() => {
    if (!categorizedProfiles) {
      return []
    }

    return [
      ...categorizedProfiles[ProfileType.JavaScript],
      ...categorizedProfiles[ProfileType.Lua],
      ...categorizedProfiles[ProfileType.Merge],
    ]
  }, [categorizedProfiles])

  const scriptProfileUids = useMemo(() => {
    return scriptProfiles.map((item) => item.uid)
  }, [scriptProfiles])

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

  // sync chains with profile.chain and scriptProfiles
  useEffect(() => {
    const activeSet = new Set(profile.chain ?? [])
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
  }, [scriptProfileUids, profile.chain])

  const isChanged = useMemo(() => {
    const activeSet = new Set(profile.chain ?? [])
    const baselineActive = scriptProfileUids.filter((uid) => activeSet.has(uid))
    const baselineInactive = scriptProfileUids.filter(
      (uid) => !activeSet.has(uid),
    )

    return (
      !hasSameOrder(chainsUids[ColumnType.Active], baselineActive) ||
      !hasSameOrder(chainsUids[ColumnType.Inactive], baselineInactive)
    )
  }, [chainsUids, profile.chain, scriptProfileUids])

  const blockTask = useBlockTask(`update-chain-${profile.uid}`, async () => {
    try {
      await patch.mutateAsync({
        uid: profile.uid,
        profile: {
          ...(profile as ProfileBuilder),
          chain: chainsUids[ColumnType.Active],
        } as ProfileBuilder,
      })
    } catch {
      //
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
