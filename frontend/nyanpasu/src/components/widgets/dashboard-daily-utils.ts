import type {
  ProfileItem_Serialize,
  ProfileSubscriptionInfo,
  RunDto,
} from '@nyanpasu/rpc/types'
import type { SubscriptionTarget } from './widget-config'

export type ProfileTargetResolution =
  | { kind: 'selected'; profile: ProfileItem_Serialize }
  | { kind: 'needs_selection' }
  | { kind: 'missing' }
  | { kind: 'unsupported'; profile?: ProfileItem_Serialize }

export const isRemoteFileConfig = (item: ProfileItem_Serialize): boolean =>
  item.type === 'config' &&
  item.config.type === 'file' &&
  item.config.source.type === 'remote'

export const isActivatableConfig = (item: ProfileItem_Serialize): boolean =>
  item.type === 'config'

export function getRemoteFileConfigs(
  items: ProfileItem_Serialize[],
  allowedUids?: string[],
): ProfileItem_Serialize[] {
  const allowed = allowedUids ? new Set(allowedUids) : undefined
  return items.filter(
    (item) => isRemoteFileConfig(item) && (!allowed || allowed.has(item.uid)),
  )
}

/** Resolve a stored target without silently substituting another profile. */
export function resolveProfileTarget(
  items: ProfileItem_Serialize[],
  currentUid: string | null | undefined,
  target: SubscriptionTarget,
): ProfileTargetResolution {
  const uid = target.kind === 'current' ? currentUid : target.profileUid
  if (!uid) {
    return target.kind === 'current'
      ? { kind: 'needs_selection' }
      : { kind: 'missing' }
  }

  const profile = items.find((item) => item.uid === uid)
  if (!profile) return { kind: 'missing' }
  if (
    target.kind === 'current' &&
    profile.type === 'config' &&
    profile.config.type === 'composition'
  ) {
    return { kind: 'needs_selection' }
  }
  if (!isRemoteFileConfig(profile)) return { kind: 'unsupported', profile }
  return { kind: 'selected', profile }
}

export type SubscriptionQuota = {
  upload: number
  download: number
  used: number
  total: number
  remaining: number
  usedPercent: number
  remainingPercent: number
  overage: number
}

const isFiniteNonNegative = (
  value: number | null | undefined,
): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0

/** Missing directions or an invalid/zero total remain unknown, not zero. */
export function calculateSubscriptionQuota(
  subscription?: ProfileSubscriptionInfo | null,
): SubscriptionQuota | null {
  const upload = subscription?.upload
  const download = subscription?.download
  const total = subscription?.total
  if (
    !subscription ||
    !isFiniteNonNegative(upload) ||
    !isFiniteNonNegative(download) ||
    !isFiniteNonNegative(total) ||
    total === 0
  ) {
    return null
  }

  const used = upload + download
  const usedPercent = (used / total) * 100

  return {
    upload,
    download,
    used,
    total,
    remaining: Math.max(0, total - used),
    usedPercent: Math.min(100, usedPercent),
    remainingPercent: Math.max(0, 100 - usedPercent),
    overage: Math.max(0, used - total),
  }
}

export function subscriptionExpiryTimestamp(
  expire?: number | null,
): number | null {
  if (typeof expire !== 'number' || !Number.isFinite(expire) || expire <= 0) {
    return null
  }
  const timestamp = expire * 1000
  return Number.isFinite(timestamp) &&
    Number.isFinite(new Date(timestamp).getTime())
    ? timestamp
    : null
}

export function isExpiryWarning(
  timestamp: number | null,
  warningDays: number,
  now: number,
): boolean {
  return (
    timestamp !== null && timestamp - now <= warningDays * 24 * 60 * 60 * 1000
  )
}

export function isQuotaWarning(
  quota: SubscriptionQuota | null,
  warningPercent: number,
): boolean {
  return quota !== null && quota.remainingPercent <= warningPercent
}

export function latestFinishedRuns(runs: RunDto[], limit: number): RunDto[] {
  return runs
    .filter((run) => run.state.kind === 'finished')
    .sort((a, b) => {
      const aTime = a.finished_at ? Date.parse(a.finished_at) : 0
      const bTime = b.finished_at ? Date.parse(b.finished_at) : 0
      if (aTime !== bTime) return bTime - aTime
      return BigInt(a.admission_sequence) > BigInt(b.admission_sequence)
        ? -1
        : BigInt(a.admission_sequence) < BigInt(b.admission_sequence)
          ? 1
          : 0
    })
    .slice(0, Math.max(0, limit))
}
