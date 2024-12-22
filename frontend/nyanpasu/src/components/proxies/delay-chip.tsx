import { memo, useState } from 'react'
import { useColorForDelay } from '@/hooks/theme'
import { Bolt } from '@mui/icons-material'
import { CircularProgress } from '@mui/material'
import { cn } from '@nyanpasu/ui'
import FeatureChip from './feature-chip'

export const DelayChip = memo(function DelayChip({
  className,
  delay,
  onClick,
}: {
  className?: string
  delay: number
  onClick: () => Promise<void>
}) {
  const [loading, setLoading] = useState(false)

  const handleClick = async () => {
    try {
      setLoading(true)

      await onClick()
    } finally {
      setLoading(false)
    }
  }

  return (
    <FeatureChip
      className={cn(className, loading && '!visible')}
      sx={{
        ml: 'auto',
        color: useColorForDelay(delay),
      }}
      label={
        <>
          <span
            className={cn(
              'flex items-center px-[1px] transition-opacity',
              loading ? 'opacity-0' : 'opacity-1',
            )}
          >
            {delay === -1 ? (
              <Bolt className="scale-[0.6]" />
            ) : !!delay && delay < 10000 ? (
              `${delay} ms`
            ) : (
              'timeout'
            )}
          </span>

          <CircularProgress
            size={12}
            className={cn(
              'transition-opacity',
              'absolute',
              'animate-spin',
              'top-0',
              'bottom-0',
              'left-0',
              'right-0',
              'm-auto',
              loading ? 'opacity-1' : 'opacity-0',
            )}
          />
        </>
      }
      variant="filled"
      onClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
        handleClick()
      }}
    />
  )
})

export default DelayChip
