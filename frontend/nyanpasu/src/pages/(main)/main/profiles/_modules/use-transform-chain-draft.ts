import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useBlockTask } from '@/components/providers/block-task-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useProfile } from '@nyanpasu/query'
import { isChainProfile, type TransformProfile } from './utils'

type DragSession = {
  uids: string[]
  profiles: TransformProfile[]
}

const sameOrder = (left: string[], right: string[]) =>
  left.length === right.length &&
  left.every((uid, index) => uid === right[index])

/**
 * Draft of an enabled transform chain. The persisted order wins until the user
 * edits it, and uids without a matching transform profile are dropped.
 */
export function useTransformChainDraft({
  transforms: currentTransforms,
  taskKey,
  onApply,
}: {
  /** The persisted chain, in order. */
  transforms: string[]
  taskKey: string
  onApply: (transforms: string[]) => Promise<unknown>
}) {
  const {
    query: { data: profiles },
  } = useProfile()

  const dataReady = profiles != null

  // Candidate chain items = every Transform profile (Overlay / Script).
  const candidates = useMemo(
    () => (profiles?.items ?? []).filter(isChainProfile),
    [profiles?.items],
  )

  const candidateUids = useMemo(
    () => candidates.map((item) => item.uid),
    [candidates],
  )

  const serverEnabled = useMemo(() => {
    const known = new Set(candidateUids)

    return currentTransforms.filter((uid) => known.has(uid))
  }, [currentTransforms, candidateUids])

  const [draft, setDraft] = useState<string[]>(serverEnabled)

  // The draft is checked alongside the config because a corrected server
  // snapshot lands one commit before the sync effect replaces the draft, and
  // two sortables sharing a uid clobber each other in the dnd-kit registry.
  const hasDuplicates =
    new Set(currentTransforms).size !== currentTransforms.length ||
    new Set(draft).size !== draft.length

  const draftRef = useRef(draft)
  // null until the first server snapshot is adopted, so the initial load wins.
  const adoptedRef = useRef<string[] | null>(null)
  const dragSessionRef = useRef<DragSession | null>(null)
  const [dragSession, setDragSession] = useState<DragSession | null>(null)
  const savingRef = useRef(false)

  const publishDraft = useCallback((next: string[]) => {
    draftRef.current = next
    setDraft(next)
  }, [])

  useEffect(() => {
    if (!dataReady || dragSessionRef.current) return

    const serverChanged =
      adoptedRef.current === null ||
      !sameOrder(adoptedRef.current, serverEnabled)

    // A changed chain is the server's call; a changed candidate list only
    // removes entries and must not discard the draft order.
    const known = new Set(candidateUids)
    const next = serverChanged
      ? serverEnabled
      : draftRef.current.filter((uid) => known.has(uid))

    adoptedRef.current = serverEnabled

    if (!sameOrder(next, draftRef.current)) {
      publishDraft(next)
    }
  }, [serverEnabled, candidateUids, dragSession, dataReady, publishDraft])

  // Snapshot the candidates for the duration of a gesture so a refetch cannot
  // unmount the element being dragged.
  const renderedProfiles = dragSession?.profiles ?? candidates

  const enabled = useMemo(() => {
    const byUid = new Map(renderedProfiles.map((item) => [item.uid, item]))

    return draft
      .map((uid) => byUid.get(uid))
      .filter((item): item is TransformProfile => Boolean(item))
  }, [draft, renderedProfiles])

  const isChanged = !sameOrder(draft, serverEnabled)

  const appliedUids = useMemo(() => new Set(serverEnabled), [serverEnabled])

  const changes = useMemo(() => {
    const draftUids = new Set(draft)

    return {
      added: [...draftUids].filter((uid) => !appliedUids.has(uid)).length,
      removed: [...appliedUids].filter((uid) => !draftUids.has(uid)).length,
      // Only the items that stay count: an add or a remove is not a reorder.
      reordered: !sameOrder(
        draft.filter((uid) => appliedUids.has(uid)),
        serverEnabled.filter((uid) => draftUids.has(uid)),
      ),
    }
  }, [draft, serverEnabled, appliedUids])

  /** Whether the uid is in the draft but not in the persisted chain. */
  const isUnapplied = useCallback(
    (uid: string) => !appliedUids.has(uid),
    [appliedUids],
  )

  const blockTask = useBlockTask(taskKey, async (transforms: string[]) => {
    try {
      await onApply(transforms)
    } catch (error) {
      message(
        m.profile_chain_editor_update_failed({ error: formatError(error) }),
        {
          title: m.profile_chain_editor_update_failed_title(),
          kind: 'error',
          error,
        },
      )
    }
  })

  const saving = blockTask.isPending
  const editable = dataReady && !hasDuplicates
  const dragging = dragSession !== null
  const actionsDisabled = !editable || saving || dragging

  const canEdit = useCallback(
    () => editable && !savingRef.current && !dragSessionRef.current,
    [editable],
  )

  const handleAdd = useCallback(
    (uid: string) => {
      if (!canEdit() || draftRef.current.includes(uid)) return

      publishDraft([...draftRef.current, uid])
    },
    [canEdit, publishDraft],
  )

  const handleRemove = useCallback(
    (uid: string) => {
      if (!canEdit()) return

      publishDraft(draftRef.current.filter((item) => item !== uid))
    },
    [canEdit, publishDraft],
  )

  const handleApply = async () => {
    if (!canEdit()) return

    savingRef.current = true

    try {
      const known = new Set(candidateUids)
      const next = draftRef.current.filter((uid) => known.has(uid))
      if (!sameOrder(next, draftRef.current)) publishDraft(next)

      await blockTask.execute(next)
    } finally {
      savingRef.current = false
    }
  }

  const handleReset = () => {
    if (!canEdit()) return

    adoptedRef.current = serverEnabled
    publishDraft(serverEnabled)
  }

  /** Whether a drag may start now; the sensors call it before and after the start. */
  const canDrag = useCallback(() => editable && !savingRef.current, [editable])

  const beginDrag = useCallback(() => {
    const session = { uids: draftRef.current, profiles: candidates }

    dragSessionRef.current = session
    setDragSession(session)
  }, [candidates])

  const reorder = useCallback(
    (update: (current: string[]) => string[]) => {
      if (!dragSessionRef.current || savingRef.current) return

      const next = update(draftRef.current)
      if (!sameOrder(next, draftRef.current)) publishDraft(next)
    },
    [publishDraft],
  )

  const endDrag = useCallback(
    (canceled: boolean) => {
      const session = dragSessionRef.current
      if (canceled && session) publishDraft(session.uids)

      dragSessionRef.current = null
      setDragSession(null)
    },
    [publishDraft],
  )

  return {
    dataReady,
    enabled,
    candidates,
    isChanged,
    changes,
    isUnapplied,
    hasDuplicates,
    editable,
    saving,
    dragging,
    actionsDisabled,
    handleAdd,
    handleRemove,
    handleApply,
    handleReset,
    canDrag,
    beginDrag,
    reorder,
    endDrag,
  }
}

export type TransformChainDraft = ReturnType<typeof useTransformChainDraft>
