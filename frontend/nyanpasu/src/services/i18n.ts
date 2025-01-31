import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from '@/locales/en.json'
import ru from '@/locales/ru.json'
import zhCN from '@/locales/zh-CN.json'
import zhTW from '@/locales/zh-TW.json'

const resources = {
  en: { translation: en },
  ru: { translation: ru },
  'zh-CN': { translation: zhCN },
  'zh-TW': { translation: zhTW },
}

i18n.use(initReactI18next).init({
  resources,
  lng: 'en',
  interpolation: {
    escapeValue: false,
  },
})
