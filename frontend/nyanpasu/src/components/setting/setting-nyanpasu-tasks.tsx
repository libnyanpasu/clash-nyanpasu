import { useTranslation } from 'react-i18next'
import { List } from '@mui/material'
import { useSetting } from '@nyanpasu/interface'
import { BaseCard, NumberItem } from '@nyanpasu/ui'

export const SettingNyanpasuTasks = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('max_log_files')

  return (
    <BaseCard label={t('Tasks')}>
      <List disablePadding>
        <NumberItem
          value={value || 0}
          label={t('Max Log Files')}
          checkEvent={(value) => value <= 0}
          checkLabel="Value must larger than 0."
          onApply={(v) => upsert(v)}
        />
      </List>
    </BaseCard>
  )
}

export default SettingNyanpasuTasks
