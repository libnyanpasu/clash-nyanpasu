import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { useClashSystemSettings, useConfigFeatures } from '@nyanpasu/query'
import type {
  DnsFeatures,
  DnsKey,
  DnsSettings,
  FallbackFilter,
  OneOrMany,
} from '@nyanpasu/rpc/types'

/**
 * The saved DNS settings. Their fields have serde defaults, so the generated
 * type marks them optional, but the backend serializes every one.
 */
export type DnsValues = Required<Omit<DnsSettings, 'fallback-filter'>> & {
  'fallback-filter': Required<FallbackFilter>
}

export type Hosts = Record<string, OneOrMany>

export type DnsDraft = {
  dns: DnsValues
  hosts: Hosts
}

type DnsDraftContextValue = {
  draft: DnsDraft
  features: DnsFeatures
  /** Whether the draft differs from the saved settings. */
  dirty: boolean
  saving: boolean
  updateDns: (patch: Partial<DnsValues>) => void
  setHosts: (hosts: Hosts) => void
  replaceDraft: (draft: DnsDraft) => void
  discard: () => void
  save: () => Promise<void>
  /** Whether the selected core reads `key`. */
  reads: (key: DnsKey) => boolean
}

const DnsDraftContext = createContext<DnsDraftContextValue | null>(null)

export const useDnsDraft = () => {
  const context = useContext(DnsDraftContext)

  if (!context) {
    throw new Error('useDnsDraft must be used within DnsDraftProvider')
  }

  return context
}

/**
 * The DNS page edits `dns` and `hosts` as one draft and saves them together,
 * so a half-edited table never reaches the core.
 */
export function DnsDraftProvider({
  saved,
  features,
  children,
}: PropsWithChildren<{ saved: DnsDraft; features: DnsFeatures }>) {
  const { upsert } = useClashSystemSettings()

  const [draft, setDraft] = useState(saved)

  // A save or another window changed the settings: show what is saved.
  useEffect(() => {
    setDraft(saved)
  }, [saved])

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(saved),
    [draft, saved],
  )

  const updateDns = useCallback(
    (patch: Partial<DnsValues>) =>
      setDraft((current) => ({
        ...current,
        dns: { ...current.dns, ...patch },
      })),
    [],
  )

  const setHosts = useCallback(
    (hosts: Hosts) => setDraft((current) => ({ ...current, hosts })),
    [],
  )

  const discard = useCallback(() => setDraft(saved), [saved])

  const save = useCallback(async () => {
    await upsert.mutateAsync({ dns: draft.dns, hosts: draft.hosts })
  }, [upsert, draft])

  const reads = useCallback(
    (key: DnsKey) =>
      features.keys.kind === 'values' && features.keys.value.includes(key),
    [features],
  )

  const value = useMemo(
    () => ({
      draft,
      features,
      dirty,
      saving: upsert.isPending,
      updateDns,
      setHosts,
      replaceDraft: setDraft,
      discard,
      save,
      reads,
    }),
    [
      draft,
      features,
      dirty,
      upsert.isPending,
      updateDns,
      setHosts,
      discard,
      save,
      reads,
    ],
  )

  return (
    <DnsDraftContext.Provider value={value}>
      {children}
    </DnsDraftContext.Provider>
  )
}

/** The saved DNS settings and the selected core's DNS features. */
export const useSavedDns = () => {
  const { query } = useClashSystemSettings()

  const features = useConfigFeatures()

  const saved = useMemo<DnsDraft | undefined>(
    () =>
      query.data?.dns && query.data.hosts
        ? { dns: query.data.dns as DnsValues, hosts: query.data.hosts }
        : undefined,
    [query.data],
  )

  return { saved, features: features.data?.dns }
}
