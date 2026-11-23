import { AnimatePresence } from 'motion/react'
import {
  DndGridItem,
  DndGridItemProps,
  useDndGridContext,
} from '@nyanpasu/ui/dnd-grid'
import { cn } from '@nyanpasu/utils'
import { WidgetComponentProps } from './consts'
import { WidgetRemoveControl } from './core/widget-controls'
import { WidgetId } from './widget-config'
import { WidgetConfigControl } from './widget-config-control'

export type WidgetItemProps = DndGridItemProps<string> &
  WidgetComponentProps & {
    widgetType: WidgetId
  }

/**
 * Legacy grid chrome used by the remaining dashboard placeholder and browser
 * tests. Migrated widgets use WidgetGridHost instead.
 */
export default function WidgetItem({
  children,
  className,
  onCloseClick,
  widgetType,
  ...props
}: WidgetItemProps) {
  const { disabled, sourceOnly, isOverlay } = useDndGridContext()
  const canEdit = !disabled && !sourceOnly && !isOverlay

  return (
    <DndGridItem {...props} className={cn('relative', className)}>
      {children}

      <AnimatePresence>
        {canEdit && (
          <WidgetRemoveControl
            key="remove-control"
            id={props.id}
            onRemove={onCloseClick}
          />
        )}
      </AnimatePresence>

      <WidgetConfigControl id={props.id} type={widgetType} isActive={canEdit} />
    </DndGridItem>
  )
}
