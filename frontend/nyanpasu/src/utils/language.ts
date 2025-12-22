import { defineCustomClientStrategy, locales } from '@/paraglide/runtime'

export const languageOptions = {
  en: 'English',
  ru: 'Русский',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
}

export const languageQuirks: {
  [key: string]: {
    drawer: {
      minWidth: number
      itemClassNames?: string
    }
  }
} = {
  en: {
    drawer: {
      minWidth: 240,
    },
  },
  ru: {
    drawer: {
      minWidth: 240,
    },
  },
  'zh-CN': {
    drawer: {
      minWidth: 180,
    },
  },
  'zh-TW': {
    drawer: {
      minWidth: 180,
    },
  },
}

export type Language = (typeof locales)[number]

export const LANGUAGE_STORAGE_KEY = 'paraglide-language-cache'

export const DEFAULT_LANGUAGE = 'en'

// encode the language storage key to avoid special characters
const CACHED_LANGUAGE_STORAGE_KEY = btoa(LANGUAGE_STORAGE_KEY)

export const setCachedLanguage = (locale: Language) => {
  localStorage.setItem(CACHED_LANGUAGE_STORAGE_KEY, locale)
}

export const getCachedLanguage = () => {
  const value = localStorage.getItem(CACHED_LANGUAGE_STORAGE_KEY)

  return value && locales.includes(value as Language)
    ? (value as Language)
    : DEFAULT_LANGUAGE
}

defineCustomClientStrategy('custom-extension', {
  getLocale: () => {
    return getCachedLanguage()
  },
  setLocale: (locale) => {
    setCachedLanguage(locale as Language)
  },
})
