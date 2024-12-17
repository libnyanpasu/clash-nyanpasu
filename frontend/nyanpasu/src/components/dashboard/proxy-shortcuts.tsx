import { useLockFn } from 'ahooks'
import { useAtomValue } from 'jotai'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { atomIsDrawer } from '@/store'
import { message } from '@/utils/notification'
import { NetworkPing, SettingsEthernet } from '@mui/icons-material'
import { Chip, Paper, type ChipProps } from '@mui/material'
import Grid from '@mui/material/Grid2'
import { useClash, useNyanpasu } from '@nyanpasu/interface'
import { PaperSwitchButton } from '../setting/modules/system-proxy'

const TitleComp = () => {
  const { t } = useTranslation()

  const { getSystemProxy } = useNyanpasu()

  const {
    getConfigs: { data: clashConfigs },
  } = useClash()

  const status = useMemo<{
    label: string
    color: ChipProps['color']
  }>(() => {
    const data = getSystemProxy.data

    if (data?.enable) {
      const port = Number(data.server.split(':')[1])

      if (port === clashConfigs?.['mixed-port']) {
        return {
          label: t('Successful'),
          color: 'success',
        }
      } else {
        return {
          label: t('Occupied'),
          color: 'warning',
        }
      }
    } else {
      return {
        label: t('Disabled'),
        color: 'error',
      }
    }
  }, [clashConfigs, getSystemProxy.data])

  return (
    <div className="flex items-center gap-2 px-1">
      <div>{t('Proxy Takeover Status')}</div>

      <Chip
        color={status.color}
        className="!h-5"
        sx={{
          span: {
            padding: '0 8px',
          },
        }}
        label={status.label}
      />
    </div>
  )
}

export const ProxyShortcuts = () => {
  const { t } = useTranslation()

  const isDrawer = useAtomValue(atomIsDrawer)

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  const handleClick = useLockFn(
    async (key: 'enable_system_proxy' | 'enable_tun_mode') => {
      try {
        await setNyanpasuConfig({
          [key]: !nyanpasuConfig?.[key],
        })
      } catch (e) {
        message(`Activation failed!`, {
          title: t('Error'),
          kind: 'error',
        })
      }
    },
  )

  return (
    <Grid
      size={{
        sm: isDrawer ? 6 : 12,
        md: 6,
        lg: 4,
        xl: 3,
      }}
    >
      <Paper className="flex !h-full flex-col justify-between gap-2 !rounded-3xl p-3">
        <TitleComp />

        <div className="flex gap-3">
          <div className="!w-full">
            <PaperSwitchButton
              checked={nyanpasuConfig?.enable_system_proxy || false}
              onClick={() => handleClick('enable_system_proxy')}
            >
              <div className="flex flex-col gap-2">
                <NetworkPing />

                <div>{t('System Proxy')}</div>
              </div>
            </PaperSwitchButton>
          </div>

          <div className="!w-full">
            <PaperSwitchButton
              checked={nyanpasuConfig?.enable_tun_mode || false}
              onClick={() => handleClick('enable_tun_mode')}
            >
              <div className="flex flex-col gap-2">
                <SettingsEthernet />

                <div>{t('TUN Mode')}</div>
              </div>
            </PaperSwitchButton>
          </div>
        </div>
      </Paper>
    </Grid>
  )
}

export default ProxyShortcuts
