import { useTranslation } from 'react-i18next'
import {
  useSetting,
  type BreakWhenModeChange,
  type BreakWhenProfileChange,
  type BreakWhenProxyChange,
} from '@nyanpasu/interface'
import { SwitchItem } from '@nyanpasu/ui'

const BreakWhenProxyChangeSetting = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('break_when_proxy_change')

  const options = {
    none: t('None'),
    chain: t('Chain'),
    all: t('All'),
  }

  return (
    <SwitchItem
      label={t('当代理切换时重载配置')}
      checked={value !== 'none'}
      onChange={() => {
        if (value === 'none') {
          upsert('all' as BreakWhenProxyChange)
        } else {
          upsert('none' as BreakWhenProxyChange)
        }
      }}
    />
  )
}

const BreakWhenProfileChangeSetting = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('break_when_profile_change')

  return (
    <SwitchItem
      label={t('当配置文件切换时重载配置')}
      checked={value !== 'off'}
      onChange={() => {
        if (value === 'off') {
          upsert('on' as BreakWhenProfileChange)
        } else {
          upsert('off' as BreakWhenProfileChange)
        }
      }}
    />
  )
}

const BreakWhenModeChangeSetting = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('break_when_mode_change')

  return (
    <SwitchItem
      label={t('当模式切换时重载配置')}
      checked={value !== 'off'}
      onChange={() => {
        if (value === 'off') {
          upsert('on' as BreakWhenModeChange)
        } else {
          upsert('off' as BreakWhenModeChange)
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
