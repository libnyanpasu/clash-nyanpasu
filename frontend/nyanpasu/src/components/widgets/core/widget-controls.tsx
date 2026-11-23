import CloseRounded from '~icons/material-symbols/close-rounded'
import { motion, useIsPresent, useReducedMotion } from 'motion/react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'

/** The remove control shared by the grid host and the legacy item wrapper. */
export function WidgetRemoveControl({
  id,
  onRemove,
}: {
  id: string
  onRemove?: (id: string) => void
}) {
  const isPresent = useIsPresent()
  const reducedMotion = useReducedMotion()

  return (
    <Button
      variant="raised"
      className={cn(
        'absolute -top-1 -right-1 z-10 size-8',
        'border-outline/30 border',
      )}
      icon
      disabled={!isPresent}
      aria-label={m.dashboard_widget_delete()}
      aria-hidden={!isPresent}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={() => onRemove?.(id)}
      asChild
    >
      <motion.button
        initial={reducedMotion ? { opacity: 1 } : { scale: 0.85, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={reducedMotion ? { opacity: 0 } : { scale: 0.85, opacity: 0 }}
        transition={{
          type: 'tween',
          duration: reducedMotion ? 0 : 0.2,
          ease: 'easeOut',
        }}
      >
        <CloseRounded className="size-4" />
      </motion.button>
    </Button>
  )
}
