import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { useNyanpasu } from '@nyanpasu/interface'
import { BaseCard, MenuItem, SwitchItem, TextItem } from '@nyanpasu/ui'
import { nyanpasu } from './modules/create-props'

const { useBooleanProps: createBooleanProps } = nyanpasu

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

        <SwitchItem
          label={t('Auto Close Connections')}
          {...createBooleanProps('auto_close_connection')}
        />

        <SwitchItem
          label={t('Enable Built-in Enhanced')}
          {...createBooleanProps('enable_builtin_enhanced')}
        />

        <SwitchItem
          label={t('Lighten Up Animation Effects')}
          {...createBooleanProps('lighten_animation_effects')}
        />

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
