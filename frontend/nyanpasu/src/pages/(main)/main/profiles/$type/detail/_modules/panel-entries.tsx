import { cva } from 'class-variance-authority'
import { useEffect, useRef } from 'react'
import { m } from '@/paraglide/messages'
import { formatDate } from '@/utils/date'
import {
  isRemoteItem,
  scopedTransformsOf,
  useProfile,
  useProfileSyncRuns,
  useProfileSyncStatus,
} from '@nyanpasu/query'
import { type ProfileItem_Serialize } from '@nyanpasu/rpc/types'
import { isChainProfile, isProxyProfile } from '../../../_modules/utils'
import { SettingsGroup } from '../../../../_modules/settings-card'
import { type DetailPanelKind } from './detail-panel'
import { DetailRow } from './detail-section'
import { mergeSyncRuns, runStatus } from './sync-history-run'

const entryRowVariants = cva([
  // The row whose panel is open reads as the selected one.
  'data-selected:bg-secondary-container data-selected:hover:bg-secondary-container',
  'dark:data-selected:bg-secondary-container dark:data-selected:hover:bg-secondary-container',
])

/** The text under a row's label, kept to one line. */
function EntrySummary({ children }: { children: string }) {
  return <span className="block truncate">{children}</span>
}

/** The enabled count and names of the persisted chain, or that none is. */
function TransformsEntry({
  profile,
  selected,
  onOpen,
}: {
  profile: ProfileItem_Serialize
  selected: boolean
  onOpen: () => void
}) {
  const {
    query: { data },
  } = useProfile()

  const names = new Map(
    (data?.items ?? [])
      .filter(isChainProfile)
      .map((item) => [item.uid, item.name]),
  )

  const enabled = scopedTransformsOf(profile).flatMap(
    (uid) => names.get(uid) ?? [],
  )

  const summary =
    data == null
      ? undefined
      : enabled.length === 0
        ? m.profile_chain_editor_empty()
        : [
            m.profile_chain_editor_enabled_count({ count: enabled.length }),
            enabled.join(', '),
          ].join(' · ')

  return (
    <DetailRow
      label={m.profile_chain_editor_title()}
      description={summary && <EntrySummary>{summary}</EntrySummary>}
      aria-current={selected || undefined}
      data-selected={selected || undefined}
      data-panel="transforms"
      className={entryRowVariants()}
      onClick={onOpen}
    />
  )
}

/** The latest run and the next scheduled sync of a remote profile. */
function SyncEntry({
  uid,
  selected,
  onOpen,
}: {
  uid: string
  selected: boolean
  onOpen: () => void
}) {
  const status = useProfileSyncStatus(uid)
  const history = useProfileSyncRuns(uid)

  const latest = mergeSyncRuns(
    history.data?.pages.flatMap((page) => page.items),
    status.data?.active,
  )[0]

  const nextRunAt = status.data?.next_run_at

  const parts = [
    latest && runStatus(latest).label,
    latest && formatDate(latest.admitted_at),
    nextRunAt && m.profile_sync_next_run({ time: formatDate(nextRunAt) }),
  ].filter(Boolean)

  const summary =
    parts.length > 0
      ? parts.join(' · ')
      : history.isPending
        ? m.profile_sync_loading()
        : m.profile_sync_empty()

  return (
    <DetailRow
      label={m.profile_sync_history_title()}
      description={<EntrySummary>{summary}</EntrySummary>}
      aria-current={selected || undefined}
      data-selected={selected || undefined}
      data-panel="sync"
      className={entryRowVariants()}
      onClick={onOpen}
    />
  )
}

/**
 * The rows that open the detail page's panels: the transform chain of a
 * config and the sync history of a remote profile.
 */
export default function PanelEntries({
  profile,
  panel,
  restoreFocus,
  onOpen,
}: {
  profile: ProfileItem_Serialize
  /** The panel that is open. */
  panel?: DetailPanelKind
  /** The panel that just closed, whose row takes the focus back. */
  restoreFocus?: DetailPanelKind
  onOpen: (panel: DetailPanelKind) => void
}) {
  const groupRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (restoreFocus) {
      groupRef.current
        ?.querySelector<HTMLElement>(`[data-panel="${restoreFocus}"]`)
        ?.focus()
    }
  }, [restoreFocus])

  const hasTransforms = isProxyProfile(profile)
  const hasSync = isRemoteItem(profile)

  if (!hasTransforms && !hasSync) return null

  // Activating the open row again would only navigate to where the page is.
  const open = (next: DetailPanelKind) => {
    if (next !== panel) onOpen(next)
  }

  return (
    <SettingsGroup ref={groupRef} data-slot="profile-detail-panel-entries">
      {hasTransforms && (
        <TransformsEntry
          profile={profile}
          selected={panel === 'transforms'}
          onOpen={() => open('transforms')}
        />
      )}

      {hasSync && (
        <SyncEntry
          uid={profile.uid}
          selected={panel === 'sync'}
          onOpen={() => open('sync')}
        />
      )}
    </SettingsGroup>
  )
}
