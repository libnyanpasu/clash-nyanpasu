import CloseRounded from '~icons/material-symbols/close-rounded'
import { AnimatePresence, motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { DndGridItem, DndGridItemProps } from '@/components/ui/dnd-grid'
import { useDndGridContext } from '@/components/ui/dnd-grid/context'
import { cn } from '@nyanpasu/ui'
import { WidgetComponentProps } from './consts'

export type WidgetItemProps = DndGridItemProps<string> & WidgetComponentProps

export default function WidgetItem({
  children,
  className,
  onCloseClick,
  ...props
}: WidgetItemProps) {
  const { disabled, sourceOnly } = useDndGridContext()

  return (
    <DndGridItem {...props} className={cn('relative', className)}>
      {children}

      <AnimatePresence>
        {!disabled && !sourceOnly && (
          <Button
            variant="raised"
            className={cn(
              'absolute -top-1 -right-1 z-10 size-8',
              'border-outline/30 border',
            )}
            icon
            onClick={() => onCloseClick?.(props.id)}
            asChild
          >
            <motion.button
              initial={{
                scale: 0.85,
                opacity: 0,
              }}
              animate={{
                scale: 1,
                opacity: 1,
              }}
              exit={{
                scale: 0.85,
                opacity: 0,
              }}
              transition={{
                type: 'tween',
                duration: 0.1,
                ease: 'easeOut',
              }}
            >
              <CloseRounded className="size-4" />
            </motion.button>
          </Button>
        )}
      </AnimatePresence>
    </DndGridItem>
  )
}
