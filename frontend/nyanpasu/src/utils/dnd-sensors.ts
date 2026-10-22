import { PointerActivationConstraints } from '@dnd-kit/dom'
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

/**
 * Sensors for items that are also links. A click must still navigate, so a
 * mouse or pen drag starts only after the pointer moved 8px; touch keeps its
 * long-press delay so a press opens the context menu instead.
 */
export const FOLDER_ITEM_DRAG_SENSORS = [
  PointerSensor.configure({
    activationConstraints: (event, source) => {
      if (event.pointerType !== 'touch') {
        return [new PointerActivationConstraints.Distance({ value: 8 })]
      }

      const defaults = PointerSensor.defaults.activationConstraints

      return typeof defaults === 'function' ? defaults(event, source) : defaults
    },
  }),
  KeyboardSensor,
]
