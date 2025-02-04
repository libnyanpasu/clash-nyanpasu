import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { NetworkStatisticWidgetConfig, useSetting } from '@nyanpasu/interface'
import { BaseCard, MenuItem } from '@nyanpasu/ui'

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
      label={t('Network Statistic Widget Variant')}
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

export const SettingNyanpasuWidget = () => {
  const { t } = useTranslation()

  return (
    <BaseCard label={t('Network Statistic Widget')}>
      <List disablePadding>
        <NetworkWidgetVariant />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuWidget
