import { locale } from 'dayjs'
import { createContext, PropsWithChildren, useContext, useEffect } from 'react'
import { useLockFn } from '@/hooks/use-lock-fn'
import { getLocale, Locale, setLocale } from '@/paraglide/runtime'
import { useSetting } from '@nyanpasu/interface'

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

export const LanguageProvider = ({ children }: PropsWithChildren) => {
  const language = useSetting('language')

  const setLanguage = useLockFn(async (value: Locale) => {
    await language.upsert(value)
    setLocale(value)
  })

  // sync dayjs locale
  useEffect(() => {
    if (language) {
      locale(language.value || 'en')
    }
  }, [language])

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
