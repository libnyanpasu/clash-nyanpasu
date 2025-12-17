import {
  AnimatePresence,
  clamp,
  domAnimation,
  LazyMotion,
  motion,
} from 'framer-motion'
import { Key, MouseEvent, useCallback, useState } from 'react'

export type RippleConfig = {
  key: Key
  x: number
  y: number
  size: number
}

export interface RippleProps {
  ripples: RippleConfig[]
  color?: string
  onClear: (key: Key) => void
}

export const Ripple = ({ ripples, color, onClear }: RippleProps) => {
  return ripples.map((ripple) => {
    const duration = clamp(
      ripple.size > 100 ? 0.6 : 0.4,
      0.01 * ripple.size,
      0.3,
    )

    return (
      <LazyMotion key={ripple.key} features={domAnimation}>
        <AnimatePresence mode="popLayout">
          <motion.span
            className="pointer-events-none absolute inset-0 z-0 origin-center overflow-hidden rounded-full"
            data-slot="ripple-item"
            initial={{ transform: 'scale(0)', opacity: 0.4 }}
            animate={{ transform: 'scale(2)', opacity: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration }}
            style={{
              backgroundColor: color ?? 'currentColor',
              top: ripple.y,
              left: ripple.x,
              width: `${ripple.size}px`,
              height: `${ripple.size}px`,
            }}
            onAnimationComplete={() => {
              onClear(ripple.key)
            }}
          />
        </AnimatePresence>
      </LazyMotion>
    )
  })
}

export const useRipple = () => {
  const [ripples, setRipples] = useState<RippleConfig[]>([])

  const onClick = useCallback((e: MouseEvent) => {
    const target = e.currentTarget

    const size = Math.max(target.clientWidth, target.clientHeight)
    const rect = target.getBoundingClientRect()

    setRipples((prev) => [
      ...prev,
      {
        key: new Date().getTime(),
        size,
        x: e.clientX - rect.left - size / 2,
        y: e.clientY - rect.top - size / 2,
      },
    ])
  }, [])

  const onClear = useCallback((key: Key) => {
    setRipples((prev) => prev.filter((ripple) => ripple.key !== key))
  }, [])

  return { ripples, onClick, onClear }
}
