import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'
import { OS } from '@/consts'
import { sleep } from '@/utils'
import { message } from '@/utils/notification'
import Grid from '@mui/material/Grid2'
import {
  collectLogs,
  openAppConfigDir,
  openAppDataDir,
  openCoreDir,
  openLogsDir,
  restartApplication,
  setCustomAppDir,
} from '@nyanpasu/interface'
import { BaseCard } from '@nyanpasu/ui'
import { open } from '@tauri-apps/plugin-dialog'
import { PaperButton } from './modules/nyanpasu-path'

export const SettingNyanpasuPath = () => {
  const { t } = useTranslation()

  const migrateAppPath = useLockFn(async () => {
    try {
      // TODO: use current app dir as defaultPath
      const selected = await open({
        directory: true,
        multiple: false,
      })

      // user cancelled the selection
      if (!selected) {
        return
      }

      if (Array.isArray(selected)) {
        message(t('Multiple directories are not supported'), {
          title: t('Error'),
          kind: 'error',
        })

        return
      }

      await setCustomAppDir(selected)

      message(t('Successfully changed the app directory'), {
        title: t('Successful'),
        kind: 'error',
      })

      await sleep(1000)

      await restartApplication()
    } catch (e) {
      message(t('Failed to migrate', { error: `${JSON.stringify(e)}` }), {
        title: t('Error'),
        kind: 'error',
      })
    }
  })

  const gridLists = [
    { label: t('Open Config Dir'), onClick: openAppConfigDir },
    { label: t('Open Data Dir'), onClick: openAppDataDir },
    OS === 'windows' && {
      label: t('Migrate App Path'),
      onClick: migrateAppPath,
    },
    { label: t('Open Core Dir'), onClick: openCoreDir },
    { label: t('Open Log Dir'), onClick: openLogsDir },
    { label: t('Collect Logs'), onClick: collectLogs },
  ].filter((x) => !!x)

  return (
    <BaseCard label={t('Path Config')}>
      <Grid container alignItems="stretch" spacing={2}>
        {gridLists.map(({ label, onClick }) => (
          <Grid
            key={label}
            size={{
              xs: 6,
              xl: 3,
            }}
          >
            <PaperButton
              label={label}
              onClick={onClick}
              sxPaper={{ height: '100%' }}
              sxButton={{ height: '100%' }}
            />
          </Grid>
        ))}
      </Grid>
    </BaseCard>
  )
}

export default SettingNyanpasuPath
