import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { List } from '@mui/material'
import { useClash, useNyanpasu } from '@nyanpasu/interface'
import { BaseCard, NumberItem, SwitchItem } from '@nyanpasu/ui'

export const SettingClashPort = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const { getConfigs, setConfigs } = useClash()

  const port = useMemo(() => {
    return (
      getConfigs.data?.['mixed-port'] ||
      nyanpasuConfig?.verge_mixed_port ||
      7890
    )
  }, [getConfigs.data, nyanpasuConfig?.verge_mixed_port])

  return (
    <BaseCard label={t('Clash Port')}>
      <List disablePadding>
        <NumberItem
          label={t('Mixed Port')}
          value={port}
          checkEvent={(input) => input > 65535 || input < 1}
          checkLabel="Port must be between 1 and 65535."
          onApply={(value) => {
            setConfigs({ 'mixed-port': value })
            setNyanpasuConfig({ verge_mixed_port: value })
          }}
        />

        <SwitchItem
          label={t('Random Port')}
          checked={nyanpasuConfig?.enable_random_port || false}
          onChange={async () => {
            try {
              await setNyanpasuConfig({
                enable_random_port: !nyanpasuConfig?.enable_random_port,
              })
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
          }}
        />
      </List>
    </BaseCard>
  )
}

export default SettingClashPort
