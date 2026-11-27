import { cva, type VariantProps } from 'class-variance-authority'
import { motion, useReducedMotion } from 'motion/react'
import { ComponentProps, useEffect, useState } from 'react'
import { cn } from '@nyanpasu/utils'

const SAMPLE_COUNT = 96
const MORPH_INTERVAL_MS = 650

function samplePolarPolygon(radiusAt: (angle: number) => number) {
  return `polygon(${Array.from({ length: SAMPLE_COUNT }, (_, index) => {
    const angle = (index / SAMPLE_COUNT) * Math.PI * 2
    const radius = radiusAt(angle)
    const x = 50 + radius * Math.cos(angle)
    const y = 50 + radius * Math.sin(angle)
    return `${x.toFixed(2)}% ${y.toFixed(2)}%`
  }).join(',')})`
}

function roundedPolygonRadius(sides: number, roundness: number, angle: number) {
  const sector = (Math.PI * 2) / sides
  const offset = (((angle % sector) + sector) % sector) - sector / 2
  const polyRadius = Math.cos(sector / 2) / Math.cos(offset)
  return 44 * (1 - roundness + roundness * Math.min(1.12, polyRadius))
}

const LOADING_SHAPES = [
  // 7-lobe soft burst
  samplePolarPolygon((angle) => 45 - 6.5 * (1 - Math.cos(7 * angle))),
  // 4-leaf clover / rounded diamond
  samplePolarPolygon((angle) => 46 - 7 * (1 - Math.cos(4 * angle))),
  // Soft pentagon
  samplePolarPolygon((angle) => roundedPolygonRadius(5, 0.38, angle)),
  // Pill / oval
  samplePolarPolygon((angle) => {
    const cos = Math.cos(angle)
    const sin = Math.sin(angle)
    return 1 / Math.hypot(cos / 47, sin / 34)
  }),
  // 9-lobe flower
  samplePolarPolygon((angle) => 45 - 5 * (1 - Math.cos(9 * angle))),
  // Soft circle
  samplePolarPolygon(() => 44),
] as const

export const loadingIndicatorVariants = cva(
  'relative inline-flex shrink-0 items-center justify-center rounded-full select-none',
  {
    variants: {
      variant: {
        default: 'text-primary bg-transparent',
        contained: 'bg-primary-container text-on-primary-container',
      },
      size: {
        sm: 'size-9',
        md: 'size-12',
        lg: 'size-16',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'md',
    },
  },
)

const loadingIndicatorShapeVariants = cva('block bg-current', {
  variants: {
    size: {
      sm: 'size-7',
      md: 'size-9.5',
      lg: 'size-12.5',
    },
  },
  defaultVariants: {
    size: 'md',
  },
})

export type LoadingIndicatorProps = ComponentProps<'div'> &
  VariantProps<typeof loadingIndicatorVariants> & {
    variant?: 'default' | 'contained'
    size?: 'sm' | 'md' | 'lg'
  }

export function LoadingIndicator({
  variant = 'default',
  size = 'md',
  className,
  ...props
}: LoadingIndicatorProps) {
  const reducedMotion = useReducedMotion()
  const [shapeIndex, setShapeIndex] = useState(0)

  useEffect(() => {
    if (reducedMotion) {
      return
    }

    const timer = window.setInterval(() => {
      setShapeIndex((prev) => (prev + 1) % LOADING_SHAPES.length)
    }, MORPH_INTERVAL_MS)

    return () => window.clearInterval(timer)
  }, [reducedMotion])

  const activeShape = reducedMotion
    ? LOADING_SHAPES[0]
    : LOADING_SHAPES[shapeIndex]

  return (
    <div
      role="progressbar"
      aria-busy="true"
      aria-valuemin={0}
      aria-valuemax={100}
      data-slot="loading-indicator"
      data-variant={variant}
      data-size={size}
      className={cn(loadingIndicatorVariants({ variant, size }), className)}
      {...props}
    >
      <motion.span
        aria-hidden="true"
        data-slot="loading-indicator-shape"
        className={loadingIndicatorShapeVariants({ size })}
        initial={false}
        animate={{
          clipPath: activeShape,
          rotate: reducedMotion ? 0 : shapeIndex * 90,
        }}
        transition={{
          clipPath: {
            duration: reducedMotion ? 0 : 0.45,
            ease: [0.2, 0, 0, 1],
          },
          rotate: {
            duration: reducedMotion ? 0 : 0.65,
            ease: [0.2, 0, 0, 1],
          },
        }}
      />
    </div>
  )
}
