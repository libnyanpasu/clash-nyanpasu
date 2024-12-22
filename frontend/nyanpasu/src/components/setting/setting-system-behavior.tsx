import { useTranslation } from 'react-i18next'
import Grid from '@mui/material/Grid2'
import { useNyanpasu } from '@nyanpasu/interface'
import { BaseCard } from '@nyanpasu/ui'
import { PaperSwitchButton } from './modules/system-proxy'

export const SettingSystemBehavior = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  return (
    <BaseCard label={t('Initiating Behavior')}>
      <Grid container spacing={2}>
        <Grid size={{ xs: 6 }}>
          <PaperSwitchButton
            label={t('Auto Start')}
            checked={nyanpasuConfig?.enable_auto_launch || false}
            onClick={() =>
              setNyanpasuConfig({
                enable_auto_launch: !nyanpasuConfig?.enable_auto_launch,
              })
            }
          />
        </Grid>

        <Grid
          size={{
            xs: 6,
          }}
        >
          <PaperSwitchButton
            label={t('Silent Start')}
            checked={nyanpasuConfig?.enable_silent_start || false}
            onClick={() =>
              setNyanpasuConfig({
                enable_silent_start: !nyanpasuConfig?.enable_silent_start,
              })
            }
          />
        </Grid>
      </Grid>
    </BaseCard>
  )
}

export default SettingSystemBehavior
