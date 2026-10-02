import CloseRounded from '~icons/material-symbols/close-rounded'
import {
  AnimatePresence,
  motion,
  useIsPresent,
  useReducedMotion,
} from 'motion/react'
import { Button } from '@nyanpasu/ui/button'
import {
  DndGridItem,
  DndGridItemProps,
  useDndGridContext,
} from '@nyanpasu/ui/dnd-grid'
import { m } from '@/paraglide/messages'
import { cn } from '@nyanpasu/utils'
import { WidgetComponentProps } from './consts'
import { WidgetId } from './widget-config'
import WidgetConfigMenu from './widget-config-menu'

export type WidgetItemProps = DndGridItemProps<string> &
  WidgetComponentProps & {
    widgetType: WidgetId
  }

function WidgetCloseControl({
  onCloseClick,
  id,
}: {
  onCloseClick?: (id: string) => void
  id: string
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
      onClick={() => onCloseClick?.(id)}
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

function AnimatedWidgetConfigMenu({
  id,
  type,
}: {
  id: string
  type: WidgetId
}) {
  const isPresent = useIsPresent()
  const reducedMotion = useReducedMotion()

  return (
    <motion.div
      className="pointer-events-none absolute inset-0"
      aria-hidden={!isPresent}
      inert={!isPresent}
      initial={reducedMotion ? false : { opacity: 0, scale: 0.92 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.92 }}
      transition={{
        type: 'tween',
        duration: reducedMotion ? 0 : 0.2,
        ease: 'easeOut',
      }}
    >
      <WidgetConfigMenu id={id} type={type} isActive={isPresent} />
    </motion.div>
  )
}

export default function WidgetItem({
  children,
  className,
  onCloseClick,
  widgetType,
  ...props
}: WidgetItemProps) {
  const { disabled, sourceOnly, isOverlay } = useDndGridContext()

  return (
    <DndGridItem {...props} className={cn('relative', className)}>
      {children}

      <AnimatePresence>
        {!disabled && !sourceOnly && !isOverlay && (
          <WidgetCloseControl
            key="close-control"
            id={props.id}
            onCloseClick={onCloseClick}
          />
        )}
      </AnimatePresence>

      <AnimatePresence>
        {!disabled && !sourceOnly && !isOverlay && (
          <AnimatedWidgetConfigMenu
            key="config-control"
            id={props.id}
            type={widgetType}
          />
        )}
      </AnimatePresence>
    </DndGridItem>
  )
}
