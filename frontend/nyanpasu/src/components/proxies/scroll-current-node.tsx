import { useTranslation } from 'react-i18next'
import { Radar } from '@mui/icons-material'
import { Tooltip } from '@mui/material'
import { MUIButton as Button } from '@nyanpasu/ui'

export const ScrollCurrentNode = ({ onClick }: { onClick?: () => void }) => {
  const { t } = useTranslation()

  return (
    <Tooltip title={t('Locate')}>
      <Button
        size="small"
        className="!size-8 !min-w-0"
        style={{
          backgroundColor:
            'color-mix(in oklab, var(--md3-color-primary) 10%, transparent)',
        }}
        onClick={onClick}
      >
        <Radar />
      </Button>
    </Tooltip>
  )
}

export default ScrollCurrentNode
