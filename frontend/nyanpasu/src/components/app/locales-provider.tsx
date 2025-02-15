import { locale } from 'dayjs'
import { changeLanguage } from 'i18next'
import { useEffect } from 'react'
import { useSetting } from '@nyanpasu/interface'

export const LocalesProvider = () => {
  const { value } = useSetting('language')

  useEffect(() => {
    if (value) {
      locale(value === 'zh' ? 'zh-cn' : value)

      changeLanguage(value)
    }
  }, [value])

  return null
}

export default LocalesProvider
