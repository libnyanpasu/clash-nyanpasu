import { motion, MotionStyle, Transition } from 'framer-motion'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'

export default function BorderBeam({
  className,
  size = 50,
  delay = 0,
  duration = 6,
  transition,
  style,
  reverse = false,
  initialOffset = 0,
}: ComponentProps<typeof motion.div> & {
  size?: number
  duration?: number
  delay?: number
  transition?: Transition
  className?: string
  reverse?: boolean
  initialOffset?: number
}) {
  return (
    <div
      className={cn(
        'pointer-events-none absolute inset-0 rounded-[inherit]',
        'border-2 border-transparent',
        'mask-[linear-gradient(transparent,transparent),linear-gradient(#000,#000)]',
        'mask-intersect [mask-clip:padding-box,border-box]',
      )}
    >
      <motion.div
        className={cn(
          'absolute aspect-square border-2 border-transparent',
          'from-primary-main via-primary-container bg-linear-to-l to-transparent',
          className,
        )}
        style={
          {
            width: size,
            offsetPath: `rect(0 auto auto 0 round ${size}px)`,
            ...style,
          } as MotionStyle
        }
        initial={{ offsetDistance: `${initialOffset}%` }}
        animate={{
          offsetDistance: reverse
            ? [`${100 - initialOffset}%`, `${-initialOffset}%`]
            : [`${initialOffset}%`, `${100 + initialOffset}%`],
        }}
        transition={{
          repeat: Infinity,
          ease: 'linear',
          duration,
          delay: -delay,
          ...transition,
        }}
      />
    </div>
  )
}
