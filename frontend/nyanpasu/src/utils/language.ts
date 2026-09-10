import { defineCustomClientStrategy, locales } from '@/paraglide/runtime'

export type Language = (typeof locales)[number]

export const LANGUAGE_STORAGE_KEY = 'paraglide-language-cache'

// encode the language storage key to avoid special characters
const CACHED_LANGUAGE_STORAGE_KEY = btoa(LANGUAGE_STORAGE_KEY)

/**
 * Spellings that older builds persisted for a language, mapped to the paraglide
 * locale. The backend now writes the canonical lowercase key and migrates
 * existing configs, but a config that has not been migrated yet — or a stale
 * localStorage cache — can still carry these.
 */
const LEGACY_LANGUAGE_ALIASES: Record<string, Language> = {
  'en-us': 'en',
}

/**
 * Resolve an arbitrary persisted language value to a paraglide locale, or
 * `null` when this build has no messages for it.
 */
export const normalizeLanguage = (value: unknown): Language | null => {
  if (typeof value !== 'string') {
    return null
  }

  const lowered = value.toLowerCase()

  return (
    locales.find((locale) => locale === lowered) ??
    LEGACY_LANGUAGE_ALIASES[lowered] ??
    null
  )
}

export const setCachedLanguage = (locale: Language) => {
  localStorage.setItem(CACHED_LANGUAGE_STORAGE_KEY, locale)
}

/**
 * The locale chosen on a previous run, or `null` on a first launch.
 *
 * Returning `null` is deliberate: it lets the paraglide strategy chain fall
 * through to `baseLocale` for the very first render, after which the
 * `LanguageProvider` applies the system-derived language the backend resolved.
 * Reporting a cached `en` here would be indistinguishable from a user who
 * actually picked English.
 */
export const getCachedLanguage = (): Language | null =>
  normalizeLanguage(localStorage.getItem(CACHED_LANGUAGE_STORAGE_KEY))

defineCustomClientStrategy('custom-extension', {
  getLocale: () => {
    return getCachedLanguage() ?? undefined
  },
  setLocale: (locale) => {
    setCachedLanguage(locale as Language)
  },
})
