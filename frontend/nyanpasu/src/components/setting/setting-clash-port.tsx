import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { List } from '@mui/material'
import { useClashConfig, useSetting } from '@nyanpasu/interface'
import { BaseCard, NumberItem, SwitchItem } from '@nyanpasu/ui'

const ClashPort = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('verge_mixed_port')

  const { query, upsert: upsertClash } = useClashConfig()

  const port = useMemo(() => {
    return query.data?.['mixed-port'] || value || 7890
  }, [query.data, value])

  return (
    <NumberItem
      label={t('Mixed Port')}
      value={port}
      checkEvent={(input) => input > 65535 || input < 1}
      checkLabel="Port must be between 1 and 65535."
      onApply={async (value) => {
        await upsertClash.mutateAsync({ 'mixed-port': value })
        await upsert(value)
      }}
    />
  )
}

const RandomPort = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('enable_random_port')

  const handleRandomPort = async () => {
    try {
      await upsert(!value)
    } catch (e) {
      message(JSON.stringify(e), {
        title: t('Error'),
        kind: 'error',
      })
    } finally {
      message(t('After restart to take effect'), {
        title: t('Successful'),
        kind: 'info',
      })
    }
  }

  return (
    <SwitchItem
      label={t('Random Port')}
      checked={value || false}
      onChange={handleRandomPort}
    />
  )
}

export const SettingClashPort = () => {
  const { t } = useTranslation()

  return (
    <BaseCard label={t('Clash Port')}>
      <List disablePadding>
        <ClashPort />

        <RandomPort />
      </List>
    </BaseCard>
  )
}

export default SettingClashPort
