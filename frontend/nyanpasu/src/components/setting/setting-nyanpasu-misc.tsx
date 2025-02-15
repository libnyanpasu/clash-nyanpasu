import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { useNyanpasu, useSetting } from '@nyanpasu/interface'
import { BaseCard, MenuItem, SwitchItem, TextItem } from '@nyanpasu/ui'

const AutoCloseConnection = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('auto_close_connection')

  return (
    <SwitchItem
      label={t('Auto Close Connections')}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  )
}

const EnableBuiltinEnhanced = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('enable_builtin_enhanced')

  return (
    <SwitchItem
      label={t('Enable Built-in Enhanced')}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  )
}

const LightenAnimationEffects = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('lighten_animation_effects')

  return (
    <SwitchItem
      label={t('Lighten Up Animation Effects')}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  )
}

export const SettingNyanpasuMisc = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const logOptions = {
    trace: 'Trace',
    debug: 'Debug',
    info: 'Info',
    warn: 'Warn',
    error: 'Error',
    silent: 'Silent',
  }

  const trayProxiesSelectorMode = {
    normal: t('Normal'),
    hidden: t('Hidden'),
    submenu: t('Submenu'),
  }

  return (
    <BaseCard label={t('Nyanpasu Setting')}>
      <List disablePadding>
        <MenuItem
          label={t('App Log Level')}
          options={logOptions}
          selected={nyanpasuConfig?.app_log_level || 'info'}
          onSelected={(value) =>
            setNyanpasuConfig({ app_log_level: value as string })
          }
        />

        <MenuItem
          label={t('Tray Proxies Selector')}
          options={trayProxiesSelectorMode}
          selected={nyanpasuConfig?.clash_tray_selector || 'normal'}
          onSelected={(value) =>
            setNyanpasuConfig({
              clash_tray_selector: value as 'normal' | 'hidden' | 'submenu',
            })
          }
        />

        <AutoCloseConnection />

        <EnableBuiltinEnhanced />

        <LightenAnimationEffects />

        <TextItem
          label={t('Default Latency Test')}
          placeholder="http://www.gstatic.com/generate_204"
          value={nyanpasuConfig?.default_latency_test || ''}
          onApply={(value) =>
            setNyanpasuConfig({ default_latency_test: value })
          }
        />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuMisc
