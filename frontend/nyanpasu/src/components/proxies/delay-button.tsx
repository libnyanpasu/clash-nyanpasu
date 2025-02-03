import { useDebounceFn, useLockFn } from 'ahooks'
import { memo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Bolt, Done } from '@mui/icons-material'
import {
  alpha,
  Button,
  CircularProgress,
  Tooltip,
  useTheme,
} from '@mui/material'
import { cn } from '@nyanpasu/ui'

export const DelayButton = memo(function DelayButton({
  onClick,
}: {
  onClick: () => Promise<void>
}) {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const [loading, setLoading] = useState(false)

  const [mounted, setMounted] = useState(false)

  const { run: runMounted, cancel: cancelMounted } = useDebounceFn(
    () => setMounted(false),
    { wait: 1000 },
  )

  const handleClick = useLockFn(async () => {
    try {
      setLoading(true)
      setMounted(true)
      cancelMounted()

      await onClick()
    } finally {
      setLoading(false)
      runMounted()
    }
  })

  const isSuccess = mounted && !loading

  return (
    <Tooltip title={t('Latency check')}>
      <Button
        className="!fixed right-8 bottom-8 z-10 size-16 !rounded-2xl backdrop-blur"
        sx={{
          boxShadow: 8,
          backgroundColor: alpha(
            palette[isSuccess ? 'success' : 'primary'].main,
            isSuccess ? 0.7 : 0.3,
          ),

          '&:hover': {
            backgroundColor: alpha(palette.primary.main, 0.45),
          },

          '&.MuiLoadingButton-loading': {
            backgroundColor: alpha(palette.primary.main, 0.15),
          },
        }}
        onClick={handleClick}
      >
        <Bolt
          className={cn(
            '!size-8',
            '!transition-opacity',
            mounted ? 'opacity-0' : 'opacity-1',
          )}
        />

        {mounted && (
          <CircularProgress
            size={32}
            className={cn(
              'transition-opacity',
              'absolute',
              loading ? 'opacity-1' : 'opacity-0',
            )}
          />
        )}

        <Done
          color="success"
          className={cn(
            '!size-8',
            'absolute',
            '!transition-opacity',
            isSuccess ? 'opacity-1' : 'opacity-0',
          )}
        />
      </Button>
    </Tooltip>
  )
})
