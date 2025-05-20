import { useTranslation } from 'react-i18next'
import { Radar } from '@mui/icons-material'
import { Button, Tooltip } from '@mui/material'
import { alpha } from '@nyanpasu/ui'

export const ScrollCurrentNode = ({ onClick }: { onClick?: () => void }) => {
  const { t } = useTranslation()

  return (
    <Tooltip title={t('Locate')}>
      <Button
        size="small"
        className="!size-8 !min-w-0"
        sx={(theme) => ({
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
        })}
        onClick={onClick}
      >
        <Radar />
      </Button>
    </Tooltip>
  )
}

export default ScrollCurrentNode
