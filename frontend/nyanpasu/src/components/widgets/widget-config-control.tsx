import {
  AnimatePresence,
  motion,
  useIsPresent,
  useReducedMotion,
} from 'motion/react'
import { WidgetId } from './widget-config'
import WidgetConfigMenu from './widget-config-menu'

function AnimatedConfigSurface({ id, type }: { id: string; type: WidgetId }) {
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

/** The animated config trigger for the grid host and the legacy item wrapper. */
export function WidgetConfigControl({
  id,
  type,
  isActive,
}: {
  id: string
  type: WidgetId
  isActive: boolean
}) {
  return (
    <AnimatePresence>
      {isActive && (
        <AnimatedConfigSurface key="config-control" id={id} type={type} />
      )}
    </AnimatePresence>
  )
}
