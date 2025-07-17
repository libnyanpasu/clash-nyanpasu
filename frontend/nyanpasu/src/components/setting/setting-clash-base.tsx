import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useCoreType } from '@/hooks/use-store'
import { formatError } from '@/utils'
import getSystem from '@/utils/get-system'
import { message } from '@/utils/notification'
import { Button, List, ListItem, ListItemText } from '@mui/material'
import {
  openUWPTool,
  useClashConfig,
  useRuntimeProfile,
  useSetting,
  type TunStack as TunStackType,
} from '@nyanpasu/interface'
import { BaseCard, MenuItem, SwitchItem } from '@nyanpasu/ui'

const isWIN = getSystem() === 'windows'

const AllowLan = () => {
  const { t } = useTranslation()

  const { query, upsert } = useClashConfig()

  const value = useMemo(() => query.data?.['allow-lan'], [query.data])

  return (
    <SwitchItem
      label={t('Allow LAN')}
      checked={value}
      onChange={async () => {
        await upsert.mutateAsync({
          'allow-lan': !value,
        })
      }}
    />
  )
}

const IPv6 = () => {
  const { t } = useTranslation()

  const { query, upsert } = useClashConfig()

  const value = useMemo(() => query.data?.['ipv6'], [query.data])

  return (
    <SwitchItem
      label={t('IPv6')}
      checked={value}
      onChange={async () => {
        await upsert.mutateAsync({
          ipv6: !value,
        })
      }}
    />
  )
}

const TunStack = () => {
  const { t } = useTranslation()

  const [coreType] = useCoreType()

  const { value, upsert: upsertTunStack } = useSetting('tun_stack')

  const { value: enableTun, upsert: upsertTun } = useSetting('enable_tun_mode')

  const runtimeProfile = useRuntimeProfile()

  const tunStackOptions = useMemo(() => {
    const options: {
      [key: string]: string
    } = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    }

    // clash not support mixed
    if (coreType === 'clash') {
      delete options.mixed
    }
    return options
  }, [coreType])

  const selected = useMemo(() => {
    const stack = value || 'gvisor'
    return stack in tunStackOptions ? stack : 'gvisor'
  }, [tunStackOptions, value])

  return (
    <MenuItem
      label={t('TUN Stack')}
      options={tunStackOptions}
      selected={selected}
      onSelected={async (value) => {
        try {
          await upsertTunStack(value as TunStackType)

          if (enableTun) {
            // just to reload clash config
            await upsertTun(true)
          }

          // need manual mutate to refetch runtime profile
          await runtimeProfile.refetch()
        } catch (error) {
          message(`Change Tun Stack failed ! \n Error: ${formatError(error)}`, {
            title: t('Error'),
            kind: 'error',
          })
        }
      }}
    />
  )
}

const LogLevel = () => {
  const { t } = useTranslation()

  const { query, upsert } = useClashConfig()

  const options = {
    debug: 'Debug',
    info: 'Info',
    warning: 'Warn',
    error: 'Error',
    silent: 'Silent',
  }

  const value = useMemo(() => query.data?.['log-level'], [query.data])

  return (
    <MenuItem
      label={t('Log Level')}
      options={options}
      selected={value ?? 'debug'}
      onSelected={async (value) => {
        await upsert.mutateAsync({
          'log-level': value as string,
        })
      }}
    />
  )
}

const UWPTool = () => {
  const { t } = useTranslation()

  const handleClick = async () => {
    try {
      await openUWPTool()
    } catch (e) {
      message(`Failed to Open UWP Tools.\n${JSON.stringify(e)}`, {
        title: t('Error'),
        kind: 'error',
      })
    }
  }

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary={t('Open UWP Tool')} />

      <Button variant="contained" onClick={handleClick}>
        {t('Open')}
      </Button>
    </ListItem>
  )
}

export const SettingClashBase = () => {
  const { t } = useTranslation()

  const [coreType] = useCoreType()

  return (
    <BaseCard label={t('Clash Setting')}>
      <List disablePadding>
        <AllowLan />

        <IPv6 />

        {coreType !== 'clash-rs' && <TunStack />}

        <LogLevel />

        {isWIN && <UWPTool />}
      </List>
    </BaseCard>
  )
}

export default SettingClashBase
