import { useTranslation } from 'react-i18next'
import { Radar } from '@mui/icons-material'
import { alpha, Button, Tooltip, useTheme } from '@mui/material'

export const ScrollCurrentNode = ({ onClick }: { onClick?: () => void }) => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  return (
    <Tooltip title={t('Locate')}>
      <Button
        size="small"
        className="!size-8 !min-w-0"
        sx={{
          backgroundColor: alpha(palette.primary.main, 0.1),
        }}
        onClick={onClick}
      >
        <Radar />
      </Button>
    </Tooltip>
  )
}

export default ScrollCurrentNode
