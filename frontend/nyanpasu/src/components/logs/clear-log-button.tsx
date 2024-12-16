import { useSetAtom } from 'jotai'
import { useTranslation } from 'react-i18next'
import { atomLogData } from '@/store'
import { Close } from '@mui/icons-material'
import { Tooltip } from '@mui/material'
import { FloatingButton } from '@nyanpasu/ui'

export const ClearLogButton = () => {
  const { t } = useTranslation()

  const setLogData = useSetAtom(atomLogData)

  const onClear = () => {
    setLogData([])
  }

  return (
    <Tooltip title={t('Clear')}>
      <FloatingButton onClick={onClear}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  )
}

export default ClearLogButton
