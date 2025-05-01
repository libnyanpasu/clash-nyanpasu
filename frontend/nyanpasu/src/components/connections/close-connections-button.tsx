import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'
import { Close } from '@mui/icons-material'
import { Tooltip } from '@mui/material'
import { useClashConnections } from '@nyanpasu/interface'
import { FloatingButton } from '@nyanpasu/ui'

export const CloseConnectionsButton = () => {
  const { t } = useTranslation()

  const { deleteConnections } = useClashConnections()

  const onCloseAll = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined)
  })

  return (
    <Tooltip title={t('Close All')}>
      <FloatingButton onClick={onCloseAll}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  )
}

export default CloseConnectionsButton
