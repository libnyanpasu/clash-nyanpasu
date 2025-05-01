import { useMemoizedFn } from 'ahooks'
import { useTransition } from 'react'
import { useTranslation } from 'react-i18next'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { LoadingButton } from '@mui/lab'
import { Button, List, ListItem, ListItemText, Typography } from '@mui/material'
import {
  restartSidecar,
  useSetting,
  useSystemService,
} from '@nyanpasu/interface'
import { BaseCard, SwitchItem } from '@nyanpasu/ui'
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from './modules/service-manual-prompt-dialog'

export const SettingSystemService = () => {
  const { t } = useTranslation()

  const { query, upsert } = useSystemService()

  const getInstallButtonString = () => {
    switch (query.data?.status) {
      case 'running':
      case 'stopped': {
        return t('uninstall')
      }

      case 'not_installed': {
        return t('install')
      }
    }
  }
  const getControlButtonString = () => {
    switch (query.data?.status) {
      case 'running': {
        return t('stop')
      }

      case 'stopped': {
        return t('start')
      }
    }
  }

  const isDisabled = query.data?.status === 'not_installed'

  const promptDialog = useServerManualPromptDialog()

  const [installOrUninstallPending, startInstallOrUninstall] = useTransition()
  const handleInstallClick = useMemoizedFn(() => {
    startInstallOrUninstall(async () => {
      try {
        switch (query.data?.status) {
          case 'running':
          case 'stopped':
            await upsert.mutateAsync('uninstall')
            break

          case 'not_installed':
            await upsert.mutateAsync('install')
            break

          default:
            break
        }
        await restartSidecar()
      } catch (e) {
        const errorMessage = `${
          query.data?.status === 'not_installed'
            ? t('Failed to install')
            : t('Failed to uninstall')
        }: ${formatError(e)}`

        message(errorMessage, {
          kind: 'error',
          title: t('Error'),
        })
        // If the installation fails, prompt the user to manually install the service
        promptDialog.show(
          query.data?.status === 'not_installed' ? 'install' : 'uninstall',
        )
      }
    })
  })

  const [serviceControlPending, startServiceControl] = useTransition()
  const handleControlClick = useMemoizedFn(() => {
    startServiceControl(async () => {
      try {
        switch (query.data?.status) {
          case 'running':
            await upsert.mutateAsync('stop')
            break

          case 'stopped':
            await upsert.mutateAsync('start')
            break

          default:
            break
        }
        await restartSidecar()
      } catch (e) {
        const errorMessage =
          query.data?.status === 'running'
            ? `Stop failed: ${formatError(e)}`
            : `Start failed: ${formatError(e)}`

        message(errorMessage, {
          kind: 'error',
          title: t('Error'),
        })
        // If start failed show a prompt to user to start the service manually
        promptDialog.show(query.data?.status === 'running' ? 'stop' : 'start')
      }
    })
  })

  const serviceMode = useSetting('enable_service_mode')

  return (
    <BaseCard label={t('System Service')}>
      <ServerManualPromptDialogWrapper />
      <List disablePadding>
        <SwitchItem
          label={t('Service Mode')}
          disabled={isDisabled}
          checked={serviceMode.value || false}
          onChange={() => serviceMode.upsert(!serviceMode.value)}
        />

        {isDisabled && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Typography>
              {t(
                'Information: To enable service mode, make sure the Clash Nyanpasu service is installed and started',
              )}
            </Typography>
          </ListItem>
        )}

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={t('Current Status', {
              status: t(`${query.data?.status}`),
            })}
          />
          <div className="flex gap-2">
            {!isDisabled && (
              <LoadingButton
                variant="contained"
                onClick={handleControlClick}
                loading={serviceControlPending}
                disabled={installOrUninstallPending || serviceControlPending}
              >
                {getControlButtonString()}
              </LoadingButton>
            )}

            <LoadingButton
              variant="contained"
              onClick={handleInstallClick}
              loading={installOrUninstallPending}
              disabled={installOrUninstallPending || serviceControlPending}
            >
              {getInstallButtonString()}
            </LoadingButton>

            {import.meta.env.DEV && (
              <Button
                variant="contained"
                onClick={() => promptDialog.show('install')}
              >
                {t('Prompt')}
              </Button>
            )}
          </div>
        </ListItem>
      </List>
    </BaseCard>
  )
}

export default SettingSystemService
