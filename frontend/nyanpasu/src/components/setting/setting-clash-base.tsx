import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useCoreType } from '@/hooks/use-store'
import getSystem from '@/utils/get-system'
import { useGlobalMutation } from '@/utils/mutation'
import { message } from '@/utils/notification'
import { Button, List, ListItem, ListItemText } from '@mui/material'
import { pullupUWPTool, useNyanpasu, VergeConfig } from '@nyanpasu/interface'
import { BaseCard, MenuItem, SwitchItem } from '@nyanpasu/ui'
import { clash } from './modules'

const { useBooleanProps: createBooleanProps, useMenuProps: createMenuProps } =
  clash

const isWIN = getSystem() === 'windows'

export const SettingClashBase = () => {
  const { t } = useTranslation()
  const [coreType] = useCoreType()
  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()
  const clickUWP = async () => {
    try {
      await pullupUWPTool()
    } catch (e) {
      message(`Failed to Open UWP Tools.\n${JSON.stringify(e)}`, {
        title: t('Error'),
        kind: 'error',
      })
    }
  }

  const tunStackOptions = useMemo(() => {
    const options: {
      [key: string]: string
    } = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    }
    if (coreType === 'clash') {
      delete options.mixed
    }
    return options
  }, [coreType])
  const tunStackSelected = useMemo(() => {
    const stack = nyanpasuConfig?.tun_stack || 'gvisor'
    return stack in tunStackOptions ? stack : 'gvisor'
  }, [nyanpasuConfig?.tun_stack, tunStackOptions])
  const mutate = useGlobalMutation()

  return (
    <BaseCard label={t('Clash Setting')}>
      <List disablePadding>
        <SwitchItem
          label={t('Allow LAN')}
          {...createBooleanProps('allow-lan')}
        />

        <SwitchItem label={t('IPv6')} {...createBooleanProps('ipv6')} />

        {coreType !== 'clash-rs' && (
          <MenuItem
            label={t('TUN Stack')}
            options={tunStackOptions}
            selected={tunStackSelected}
            onSelected={(value) => {
              const payload = {
                tun_stack: value as NonNullable<VergeConfig['tun_stack']>,
              } as Partial<VergeConfig>
              if (nyanpasuConfig?.enable_tun_mode) {
                payload.enable_tun_mode = true // just to reload clash config
              }
              setNyanpasuConfig(payload)
              mutate(
                (key) =>
                  typeof key === 'string' &&
                  key.includes('/getRuntimeConfigYaml'),
              )
            }}
          />
        )}
        <MenuItem
          label={t('Log Level')}
          {...createMenuProps('log-level', {
            options: {
              debug: 'Debug',
              info: 'Info',
              warning: 'Warn',
              error: 'Error',
              silent: 'Silent',
            },
            fallbackSelect: 'debug',
          })}
        />

        {isWIN && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <ListItemText primary={t('Open UWP Tool')} />

            <Button variant="contained" onClick={clickUWP}>
              {t('Open')}
            </Button>
          </ListItem>
        )}
      </List>
    </BaseCard>
  )
}

export default SettingClashBase
