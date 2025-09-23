import { useDebounceFn, useLockFn } from 'ahooks'
import { memo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Bolt, Done } from '@mui/icons-material'
import { CircularProgress, Tooltip } from '@mui/material'
import { alpha, MUIButton as Button, cn } from '@nyanpasu/ui'

export const DelayButton = memo(function DelayButton({
  onClick,
}: {
  onClick: () => Promise<void>
}) {
  const { t } = useTranslation()

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
        style={{
          // Approximate previous box shadow using MD3 elevation token
          boxShadow: 'var(--md3-elevation-4)',
          // Background uses success/primary color containers to reflect state
          backgroundColor: isSuccess
            ? 'color-mix(in oklab, var(--md3-color-success, var(--md3-color-primary)) 70%, transparent)'
            : 'color-mix(in oklab, var(--md3-color-primary) 30%, transparent)',
        }}
        onClick={handleClick}
      >
        <Bolt
          className={cn(
            '!size-8',
            '!transition-opacity',
            mounted ? 'opacity-0' : 'opacity-100',
          )}
        />

        {mounted && (
          <CircularProgress
            size={32}
            className={cn(
              'transition-opacity',
              'absolute',
              loading ? 'opacity-100' : 'opacity-0',
            )}
          />
        )}

        <Done
          color="success"
          className={cn(
            '!size-8',
            'absolute',
            '!transition-opacity',
            isSuccess ? 'opacity-100' : 'opacity-0',
          )}
        />
      </Button>
    </Tooltip>
  )
})
