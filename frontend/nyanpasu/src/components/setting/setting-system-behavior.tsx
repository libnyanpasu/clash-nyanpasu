import { useTranslation } from 'react-i18next'
import Grid from '@mui/material/Grid'
import { useSetting } from '@nyanpasu/interface'
import { BaseCard } from '@nyanpasu/ui'
import { PaperSwitchButton } from './modules/system-proxy'

export const SettingSystemBehavior = () => {
  const { t } = useTranslation()

  const autoLaunch = useSetting('enable_auto_launch')

  const silentStart = useSetting('enable_silent_start')

  return (
    <BaseCard label={t('Initiating Behavior')}>
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <PaperSwitchButton
            label={t('Auto Start')}
            checked={autoLaunch.value || false}
            onClick={() => autoLaunch.upsert(!autoLaunch.value)}
          />
        </Grid>

        <Grid
          size={{
            xs: 6,
          }}
        >
          <PaperSwitchButton
            label={t('Silent Start')}
            checked={silentStart.value || false}
            onClick={() => silentStart.upsert(!silentStart.value)}
          />
        </Grid>
      </Grid>
    </BaseCard>
  )
}

export default SettingSystemBehavior
