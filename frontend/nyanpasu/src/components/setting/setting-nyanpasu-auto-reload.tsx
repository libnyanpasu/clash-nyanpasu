import { useTranslation } from 'react-i18next'
import { useSetting } from '@nyanpasu/interface'
import { SwitchItem } from '@nyanpasu/ui'

// 定义各语言的翻译文本
const translations = {
  'zh-CN': {
    proxy: '当代理切换时打断连接',
    profile: '当配置文件切换时打断连接',
    mode: '当模式切换时打断连接',
  },
  'zh-TW': {
    proxy: '當代理切換時打斷連線',
    profile: '當設定檔切換時打斷連線',
    mode: '當模式切換時打斷連線',
  },
  ru: {
    proxy: 'Прерывать соединения при смене прокси',
    profile: 'Прерывать соединения при смене профиля',
    mode: 'Прерывать соединения при смене режима',
  },
  en: {
    proxy: 'Interrupt connections when proxy changes',
    profile: 'Interrupt connections when profile changes',
    mode: 'Interrupt connections when mode changes',
  },
  // 默认使用英文
  default: {
    proxy: 'Interrupt connections when proxy changes',
    profile: 'Interrupt connections when profile changes',
    mode: 'Interrupt connections when mode changes',
  },
}

const BreakWhenProxyChangeSetting = () => {
  const { i18n } = useTranslation()
  const currentLang = i18n.language

  // 获取当前语言的翻译，如果找不到则使用默认英文
  const currentTranslations =
    translations[currentLang as keyof typeof translations] ||
    translations.default

  const { value, upsert } = useSetting('break_when_proxy_change' as any)

  return (
    <SwitchItem
      label={currentTranslations.proxy}
      checked={value !== 'none'}
      onChange={() => {
        if (value === 'none') {
          upsert('all' as any)
        } else {
          upsert('none' as any)
        }
      }}
    />
  )
}

const BreakWhenProfileChangeSetting = () => {
  const { i18n } = useTranslation()
  const currentLang = i18n.language

  // 获取当前语言的翻译，如果找不到则使用默认英文
  const currentTranslations =
    translations[currentLang as keyof typeof translations] ||
    translations.default

  const { value, upsert } = useSetting('break_when_profile_change' as any)

  return (
    <SwitchItem
      label={currentTranslations.profile}
      checked={value === true}
      onChange={() => {
        if (value === true) {
          upsert(false as any)
        } else {
          upsert(true as any)
        }
      }}
    />
  )
}

const BreakWhenModeChangeSetting = () => {
  const { i18n } = useTranslation()
  const currentLang = i18n.language

  // 获取当前语言的翻译，如果找不到则使用默认英文
  const currentTranslations =
    translations[currentLang as keyof typeof translations] ||
    translations.default

  const { value, upsert } = useSetting('break_when_mode_change' as any)

  return (
    <SwitchItem
      label={currentTranslations.mode}
      checked={value === true}
      onChange={() => {
        if (value === true) {
          upsert(false as any)
        } else {
          upsert(true as any)
        }
      }}
    />
  )
}

export {
  BreakWhenProxyChangeSetting,
  BreakWhenProfileChangeSetting,
  BreakWhenModeChangeSetting,
}
