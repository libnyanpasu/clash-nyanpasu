import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import {
  LoggingLevel,
  ProxiesSelectorMode,
  useSetting,
} from '@nyanpasu/interface'
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

const AppLogLevel = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('app_log_level')

  const logOptions = {
    trace: 'Trace',
    debug: 'Debug',
    info: 'Info',
    warn: 'Warn',
    error: 'Error',
    silent: 'Silent',
  }

  return (
    <MenuItem
      label={t('App Log Level')}
      options={logOptions}
      selected={value || 'info'}
      onSelected={(value) => upsert(value as LoggingLevel)}
    />
  )
}

const TrayProxiesSelector = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('clash_tray_selector')

  const trayProxiesSelectorMode = {
    normal: t('Normal'),
    hidden: t('Hidden'),
    submenu: t('Submenu'),
  }

  return (
    <MenuItem
      label={t('Tray Proxies Selector')}
      options={trayProxiesSelectorMode}
      selected={value || 'normal'}
      onSelected={(value) => upsert(value as ProxiesSelectorMode)}
    />
  )
}

const DefaultLatencyTest = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('default_latency_test')

  return (
    <TextItem
      label={t('Default Latency Test')}
      placeholder="http://www.gstatic.com/generate_204"
      value={value || ''}
      onApply={(value) => upsert(value)}
    />
  )
}

export const SettingNyanpasuMisc = () => {
  const { t } = useTranslation()

  return (
    <BaseCard label={t('Nyanpasu Setting')}>
      <List disablePadding>
        <AppLogLevel />

        <TrayProxiesSelector />

        <AutoCloseConnection />

        <EnableBuiltinEnhanced />

        <LightenAnimationEffects />

        <DefaultLatencyTest />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuMisc
