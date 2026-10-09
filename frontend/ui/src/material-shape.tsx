import { motion, useReducedMotion } from 'motion/react'
import type { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'

function cookiePolygon(depth: number) {
  return `polygon(${Array.from({ length: 96 }, (_, index) => {
    const angle = (index / 96) * Math.PI * 2
    const radius = 47 - (depth * (1 - Math.cos(6 * angle))) / 2
    return `${50 + radius * Math.cos(angle)}% ${50 + radius * Math.sin(angle)}%`
  }).join(',')})`
}

const COOKIE = cookiePolygon(8)
const SOFT_COOKIE = cookiePolygon(2)
const SQUARE = `polygon(${Array.from({ length: 96 }, (_, index) => {
  const angle = (index / 96) * Math.PI * 2
  const radius =
    50 / Math.max(Math.abs(Math.cos(angle)), Math.abs(Math.sin(angle)))
  return `${50 + radius * Math.cos(angle)}% ${50 + radius * Math.sin(angle)}%`
}).join(',')})`

export function MaterialShape({
  active = true,
  depth,
  inactiveSquare = false,
  className,
  ...props
}: ComponentProps<typeof motion.span> & {
  active?: boolean
  depth?: number
  inactiveSquare?: boolean
}) {
  const reducedMotion = useReducedMotion()
  return (
    <motion.span
      {...props}
      data-slot="material-shape"
      className={cn(
        'relative inline-flex items-center justify-center',
        className,
      )}
      initial={false}
      animate={{
        clipPath:
          depth === undefined
            ? active
              ? COOKIE
              : inactiveSquare
                ? SQUARE
                : SOFT_COOKIE
            : cookiePolygon(depth),
      }}
      transition={{
        duration: reducedMotion ? 0 : 0.4,
        ease: [0.22, 1, 0.36, 1],
      }}
    />
  )
}
