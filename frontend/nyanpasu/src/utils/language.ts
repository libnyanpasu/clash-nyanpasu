import {
  defineCustomClientStrategy,
  getLocale,
  locales,
  overwriteGetLocale,
  type Locale,
} from '@/paraglide/runtime'

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

const applyDocumentLanguage = (locale: string) => {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = locale.toLowerCase()
  }
}

let resolved: Locale | undefined

defineCustomClientStrategy('custom-extension', {
  getLocale: () => {
    const locale = getCachedLanguage() ?? undefined
    if (locale) applyDocumentLanguage(locale)
    return locale
  },
  setLocale: (locale) => {
    resolved = locale as Language
    setCachedLanguage(resolved)
    applyDocumentLanguage(resolved)
  },
})

// Every message resolves the locale, and a list renders several messages per
// row. Paraglide's resolution reads the URL, walks the strategies, reads the
// storage and writes the document language each time, so it runs once here.
// Every later change goes through `setLocale`, which keeps `resolved`; a
// change made in another window reaches this one through the settings, as
// `LanguageProvider` describes.
const resolveLocale = getLocale

overwriteGetLocale(() => (resolved ??= resolveLocale()))
