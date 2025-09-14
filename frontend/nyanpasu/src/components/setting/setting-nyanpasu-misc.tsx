import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import {
  LoggingLevel,
  ProxiesSelectorMode,
  useSetting,
  type NetworkStatisticWidgetConfig,
} from '@nyanpasu/interface'
import { BaseCard, MenuItem, SwitchItem, TextItem } from '@nyanpasu/ui'
import {
  BreakWhenModeChangeSetting,
  BreakWhenProfileChangeSetting,
  BreakWhenProxyChangeSetting,
} from './setting-nyanpasu-auto-reload'

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

const NetworkWidgetVariant = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('network_statistic_widget')

  const options = {
    disabled: t('Disabled'),
    small: 'Small',
    large: 'Large',
  }

  const mapping: { [key: string]: NetworkStatisticWidgetConfig } = {
    disabled: {
      kind: 'disabled',
    },
    small: {
      kind: 'enabled',
      value: 'small',
    },
    large: {
      kind: 'enabled',
      value: 'large',
    },
  }

  return (
    <MenuItem
      label={t('Network Statistic Widget')}
      options={options}
      selected={
        Object.entries(mapping).find(([_, config]) =>
          config.kind === 'disabled'
            ? value?.kind === 'disabled'
            : value?.kind === 'enabled' && config.value === value.value,
        )?.[0] || 'disabled'
      }
      onSelected={(val) => upsert(mapping[val as string])}
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

        <NetworkWidgetVariant />

        <AutoCloseConnection />

        <EnableBuiltinEnhanced />

        <LightenAnimationEffects />

        <BreakWhenProxyChangeSetting />

        <BreakWhenProfileChangeSetting />

        <BreakWhenModeChangeSetting />

        <DefaultLatencyTest />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuMisc
