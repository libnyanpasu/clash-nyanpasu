import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { ComponentProps, ReactNode } from 'react'
import { cn } from '@nyanpasu/utils'

export type ActionSwapProps = Omit<ComponentProps<'div'>, 'children'> & {
  /** Changing the key rolls the current children out and the next ones in. */
  contentKey: string | null
  children: ReactNode
}

export function ActionSwap({
  className,
  contentKey,
  children,
  ...props
}: ActionSwapProps) {
  const reduceMotion = useReducedMotion()

  return (
    <div
      data-slot="action-swap"
      {...props}
      className={cn('grid overflow-hidden', className)}
    >
      <AnimatePresence mode="popLayout" initial={false}>
        {contentKey != null && (
          <motion.div
            key={contentKey}
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
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

export type ActionSwapTextProps = Omit<ComponentProps<'div'>, 'children'> & {
  value: string | null
}

export function ActionSwapText({
  className,
  value,
  ...props
}: ActionSwapTextProps) {
  return (
    <ActionSwap
      {...props}
      className={className}
      contentKey={value}
      data-slot="action-swap-text"
    >
      {value}
    </ActionSwap>
  )
}
