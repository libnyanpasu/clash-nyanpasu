import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

export type ActionSwapTextProps = Omit<ComponentProps<'div'>, 'children'> & {
  value: string | null
}

export function ActionSwapText({
  className,
  value,
  ...props
}: ActionSwapTextProps) {
  const reduceMotion = useReducedMotion()

  return (
    <div
      {...props}
      className={cn('grid overflow-hidden', className)}
      data-slot="action-swap-text"
    >
      <AnimatePresence mode="popLayout" initial={false}>
        {value != null && (
          <motion.span
            key={value}
            className="col-start-1 row-start-1"
            initial={{
              opacity: 0,
              filter: reduceMotion ? 'blur(0px)' : 'blur(4px)',
              y: reduceMotion ? 0 : 8,
            }}
            animate={{ opacity: 1, filter: 'blur(0px)', y: 0 }}
            exit={{
              opacity: 0,
              filter: reduceMotion ? 'blur(0px)' : 'blur(4px)',
              y: reduceMotion ? 0 : -8,
            }}
            transition={
              reduceMotion
                ? { duration: 0 }
                : { duration: 0.18, ease: 'easeOut' }
            }
          >
            {value}
          </motion.span>
        )}
      </AnimatePresence>
    </div>
  )
}
