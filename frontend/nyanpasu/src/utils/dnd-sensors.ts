import { KeyboardSensor, PointerSensor } from '@dnd-kit/react'

/**
 * Sensors for lists whose whole item is the drag source. Without a drag handle
 * the default mouse activation requires a press-and-hold; touch keeps its delay
 * to stay scrollable.
 */
export const WHOLE_ITEM_DRAG_SENSORS = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType === 'mouse') return undefined

      const defaults = PointerSensor.defaults.activationConstraints

      return typeof defaults === 'function' ? defaults(event, source) : defaults
    },
  }),
  KeyboardSensor,
]
