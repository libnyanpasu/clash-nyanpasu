import { locale } from 'dayjs'
import {
  createContext,
  PropsWithChildren,
  useContext,
  useEffect,
  useState,
} from 'react'
import { useLockFn } from '@/hooks/use-lock-fn'
import { getLocale, Locale, setLocale } from '@/paraglide/runtime'
import { getCachedLanguage, normalizeLanguage } from '@/utils/language'
import { useSettings } from '@nyanpasu/interface'

const LanguageContext = createContext<{
  language?: Locale
  setLanguage: (value: Locale) => Promise<void>
} | null>(null)

export const useLanguage = () => {
  const context = useContext(LanguageContext)

  if (!context) {
    throw new Error('useLanguage must be used within a LanguageProvider')
  }

  return context
}

/**
 * Applies the language the backend persisted, which is derived from the system
 * locale on a first run and from the user's choice afterwards.
 *
 * The locale cached in `localStorage` is only a fast path that lets the tree
 * mount before the settings query resolves; the backend stays authoritative and
 * corrects the cache whenever the two disagree.
 */
export const LanguageProvider = ({ children }: PropsWithChildren) => {
  const { query, upsert } = useSettings()

  const persisted = normalizeLanguage(query.data?.language)
  const settled = query.isSuccess || query.isError

  // Deliberately compares against the cache rather than `getLocale()`: the
  // first `getLocale()` call writes whatever it resolved back into the cache,
  // so calling it here would mask a first launch (no cache at all) as a
  // deliberate choice of `baseLocale`.
  const [cached] = useState(getCachedLanguage)
  const [applied, setApplied] = useState(cached !== null)

  useEffect(() => {
    if (applied || !settled) {
      return
    }

    if (persisted && persisted !== cached) {
      setLocale(persisted, { reload: false })
    }

    // Released even when the query failed or named a language this build has no
    // messages for: `baseLocale` is then the honest answer, and the window must
    // not stay hidden waiting for a better one.
    setApplied(true)
  }, [applied, settled, persisted, cached])

  // A cache that disagrees with the backend means the cache is stale — a locale
  // paraglide auto-persisted before the backend answered on an earlier run, or
  // a change made in another window. Paraglide reads messages at call time, so
  // a reload is what makes an already-mounted tree switch language.
  useEffect(() => {
    if (!applied || !persisted || persisted === getLocale()) {
      return
    }

    setLocale(persisted)
  }, [applied, persisted])

  const setLanguage = useLockFn(async (value: Locale) => {
    await upsert.mutateAsync({ language: value })
    setLocale(value)
  })

  // sync dayjs locale
  useEffect(() => {
    if (!applied) {
      return
    }

    locale(persisted ?? getLocale())
  }, [applied, persisted])

  if (!applied) {
    return null
  }

  return (
    <LanguageContext.Provider
      value={{
        language: getLocale(),
        setLanguage,
      }}
    >
      {children}
    </LanguageContext.Provider>
  )
}
