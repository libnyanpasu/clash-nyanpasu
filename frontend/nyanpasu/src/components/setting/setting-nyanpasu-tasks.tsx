import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { useNyanpasu } from '@nyanpasu/interface'
import { BaseCard, NumberItem } from '@nyanpasu/ui'

export const SettingNyanpasuTasks = () => {
  const { t } = useTranslation()

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu()

  return (
    <BaseCard label={t('Tasks')}>
      <List disablePadding>
        <NumberItem
          value={nyanpasuConfig?.max_log_files || 0}
          label={t('Max Log Files')}
          checkEvent={(value) => value <= 0}
          checkLabel="Value must larger than 0."
          onApply={(value) => setNyanpasuConfig({ max_log_files: value })}
        />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuTasks
