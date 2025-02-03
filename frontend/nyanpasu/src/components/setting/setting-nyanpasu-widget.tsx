import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { StatisticWidgetVariant, useSetting } from '@nyanpasu/interface'
import { BaseCard, Expand, MenuItem, SwitchItem } from '@nyanpasu/ui'

const NetworkWidgetEnable = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('network_statistic_widget')

  return (
    <SwitchItem
      label={t('Enable')}
      checked={value?.kind === 'enabled'}
      onChange={async () =>
        await upsert({
          kind: value?.kind === 'enabled' ? 'disabled' : 'enabled',
          value: 'small',
        })
      }
    />
  )
}

const NetworkWidgetVariant = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('network_statistic_widget')

  return (
    <Expand open={value?.kind === 'enabled'}>
      <MenuItem
        label={t('Network Statistic Widget Variant')}
        options={{
          small: 'Small',
          large: 'Large',
        }}
        selected={value?.kind === 'enabled' ? value.value : 'small'}
        onSelected={(val) =>
          upsert({ kind: 'enabled', value: val as StatisticWidgetVariant })
        }
      />
    </Expand>
  )
}

export const SettingNyanpasuWidget = () => {
  const { t } = useTranslation()

  return (
    <BaseCard label={t('Network Statistic Widget')}>
      <List disablePadding>
        <NetworkWidgetEnable />

        <NetworkWidgetVariant />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuWidget
