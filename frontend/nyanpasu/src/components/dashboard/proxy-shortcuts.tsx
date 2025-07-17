import { useLockFn } from 'ahooks'
import { useAtomValue } from 'jotai'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { atomIsDrawer } from '@/store'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { NetworkPing, SettingsEthernet } from '@mui/icons-material'
import { Chip, Paper, type ChipProps } from '@mui/material'
import Grid from '@mui/material/Grid'
import { useClashConfig, useSetting, useSystemProxy } from '@nyanpasu/interface'
import { PaperSwitchButton } from '../setting/modules/system-proxy'

const TitleComp = () => {
  const { t } = useTranslation()

  const { data } = useSystemProxy()

  const {
    query: { data: clashConfigs },
  } = useClashConfig()

  const status = useMemo<{
    label: string
    color: ChipProps['color']
  }>(() => {
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
  }, [clashConfigs, data?.enable, data?.server, t])

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

  const systemProxy = useSetting('enable_system_proxy')

  const handleSystemProxy = useLockFn(async () => {
    try {
      await systemProxy.upsert(!systemProxy.value)
    } catch (error) {
      message(`Activation System Proxy failed!`, {
        title: t('Error'),
        kind: 'error',
      })
    }
  })

  const tunMode = useSetting('enable_tun_mode')

  const handleTunMode = useLockFn(async () => {
    try {
      await tunMode.upsert(!tunMode.value)
    } catch (error) {
      message(`Activation TUN Mode failed! \n Error: ${formatError(error)}`, {
        title: t('Error'),
        kind: 'error',
      })
    }
  })

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
              checked={systemProxy.value || false}
              onClick={handleSystemProxy}
            >
              <div className="flex flex-col gap-2">
                <NetworkPing />

                <div>{t('System Proxy')}</div>
              </div>
            </PaperSwitchButton>
          </div>

          <div className="!w-full">
            <PaperSwitchButton
              checked={tunMode.value || false}
              onClick={handleTunMode}
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
