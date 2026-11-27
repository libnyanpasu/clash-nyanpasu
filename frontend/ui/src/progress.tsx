import { cva, type VariantProps } from 'class-variance-authority'
import { useReducedMotion } from 'motion/react'
import { ComponentProps, useEffect, useId, useState } from 'react'
import { cn } from '@nyanpasu/utils'
import { CircleSVG } from './circle'

const VIEWBOX_SIZE = 100
const WAVE_PATTERN_WIDTH = 24
const CIRCULAR_GAP_PX = 8

const STROKE_WIDTH_MAP = {
  thin: 10,
  thick: 16,
} as const

const LINEAR_WAVE_HEIGHT_MAP = {
  sm: 10,
  md: 16,
  lg: 20,
} as const

const LINEAR_STROKE_WIDTH_MAP = {
  sm: 3,
  md: 4,
  lg: 5,
} as const

export const circularProgressVariants = cva(
  'text-primary relative inline-flex shrink-0 items-center justify-center overflow-hidden',
  {
    variants: {
      size: {
        sm: 'size-6',
        md: 'size-10',
        lg: 'size-12',
      },
    },
    defaultVariants: {
      size: 'lg',
    },
  },
)

function clampProgress(value: number | undefined, fallback = 0) {
  return Math.min(100, Math.max(0, value ?? fallback))
}

function easeInOutCubic(t: number) {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2
}

function buildWavyCirclePath({
  radius,
  amplitude,
  waves,
  startFraction,
  endFraction,
  phase,
}: {
  radius: number
  amplitude: number
  waves: number
  startFraction: number
  endFraction: number
  phase: number
}) {
  const span = Math.max(0, Math.min(1, endFraction - startFraction))

  if (span <= 0) {
    return ''
  }

  const center = VIEWBOX_SIZE / 2
  const steps = Math.max(24, Math.ceil(span * 180))
  const points: string[] = []

  for (let i = 0; i <= steps; i++) {
    const fraction = startFraction + (span * i) / steps
    const angle = fraction * Math.PI * 2 - Math.PI / 2
    const r = radius + amplitude * Math.sin(waves * angle + phase)
    const x = center + r * Math.cos(angle)
    const y = center + r * Math.sin(angle)
    points.push(`${i === 0 ? 'M' : 'L'} ${x.toFixed(2)} ${y.toFixed(2)}`)
  }

  return points.join(' ')
}

export type CircularProgressProps = ComponentProps<'div'> &
  VariantProps<typeof circularProgressVariants> & {
    indeterminate?: boolean
    value?: number
    variant?: 'flat' | 'wavy'
    thickness?: 'thin' | 'thick'
    showTrack?: boolean
    trackClassName?: string
    indicatorClassName?: string
  }

export function CircularProgress({
  value,
  indeterminate = false,
  variant = 'flat',
  size = 'lg',
  thickness = 'thin',
  showTrack = true,
  className,
  trackClassName,
  indicatorClassName,
  children,
  ...props
}: CircularProgressProps) {
  const reducedMotion = useReducedMotion()
  const [elapsedSec, setElapsedSec] = useState(0)

  const clampedValue = clampProgress(value, 100)
  const strokeWidth = STROKE_WIDTH_MAP[thickness]
  const waveAmplitude =
    variant === 'wavy' ? (thickness === 'thick' ? 3 : 3.5) : 0
  const radius = (VIEWBOX_SIZE - strokeWidth) / 2 - waveAmplitude
  const circumference = 2 * Math.PI * radius
  const centerlineGap = strokeWidth + CIRCULAR_GAP_PX

  useEffect(() => {
    if (variant !== 'wavy' || reducedMotion) {
      return
    }
    if (!indeterminate && (clampedValue <= 0 || clampedValue >= 100)) {
      return
    }

    let frameId = 0
    const startTime = performance.now()

    const tick = (now: number) => {
      setElapsedSec((now - startTime) / 1000)
      frameId = requestAnimationFrame(tick)
    }

    frameId = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frameId)
  }, [variant, indeterminate, clampedValue, reducedMotion])

  const wavePhase = -(elapsedSec * 4.5)
  const activeArcLength = (clampedValue / 100) * circumference
  const hasGap = clampedValue > 0 && clampedValue < 100
  const trackArcLength =
    clampedValue >= 100
      ? 0
      : hasGap
        ? Math.max(0, circumference - activeArcLength - 2 * centerlineGap)
        : circumference
  const trackDashOffset = hasGap ? -(activeArcLength + centerlineGap) : 0

  // Indeterminate wavy arc crawls around the circle: head extends ahead, then tail catches up.
  const stridePeriod = 1.6
  const strideIndex = Math.floor(elapsedSec / stridePeriod)
  const strideU = (elapsedSec % stridePeriod) / stridePeriod
  const headEase = easeInOutCubic(Math.min(1, strideU / 0.65))
  const tailEase = easeInOutCubic(Math.max(0, (strideU - 0.35) / 0.65))
  const strideBase = strideIndex * 0.72 + elapsedSec * 0.16
  const wavyStartFraction = reducedMotion ? 0 : strideBase + tailEase * 0.72
  const wavyEndFraction = reducedMotion
    ? 0.58
    : strideBase + 0.1 + headEase * 0.72
  const wavyIndeterminateSpan = Math.max(
    0,
    Math.min(1, wavyEndFraction - wavyStartFraction),
  )
  const wavyIndeterminateTrackLength = Math.max(
    0,
    circumference * (1 - wavyIndeterminateSpan) - 2 * centerlineGap,
  )
  const wavyIndeterminateTrackRotation =
    (wavyEndFraction + centerlineGap / circumference) * 360 - 90

  const wavyDeterminatePath =
    variant === 'wavy' && !indeterminate && clampedValue > 0
      ? buildWavyCirclePath({
          radius,
          amplitude: waveAmplitude,
          waves: 10,
          startFraction: 0,
          endFraction: clampedValue / 100,
          phase: wavePhase,
        })
      : ''

  const wavyIndeterminatePath =
    variant === 'wavy' && indeterminate
      ? buildWavyCirclePath({
          radius,
          amplitude: waveAmplitude,
          waves: 10,
          startFraction: wavyStartFraction,
          endFraction: wavyEndFraction,
          phase: wavePhase,
        })
      : ''

  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : clampedValue}
      data-slot="circular-progress"
      data-variant={variant}
      data-indeterminate={indeterminate ? 'true' : undefined}
      className={cn(circularProgressVariants({ size }), className)}
      {...props}
    >
      {indeterminate ? (
        variant === 'wavy' ? (
          <div
            className="absolute inset-0"
            data-slot="circular-progress-indeterminate"
          >
            <CircleSVG data-slot="circular-progress-indeterminate-svg">
              {showTrack && wavyIndeterminateTrackLength > 0 && (
                <circle
                  cx="50%"
                  cy="50%"
                  r={radius}
                  strokeWidth={strokeWidth}
                  strokeLinecap="round"
                  transform={`rotate(${wavyIndeterminateTrackRotation} ${VIEWBOX_SIZE / 2} ${VIEWBOX_SIZE / 2})`}
                  style={{
                    strokeDasharray: `${wavyIndeterminateTrackLength} ${circumference}`,
                    strokeDashoffset: 0,
                  }}
                  className={cn(
                    'stroke-surface-container-highest fill-transparent',
                    trackClassName,
                  )}
                  data-slot="circular-progress-track"
                />
              )}

              <path
                d={wavyIndeterminatePath}
                strokeWidth={strokeWidth}
                strokeLinecap="round"
                className={cn(
                  'fill-transparent stroke-current',
                  indicatorClassName,
                )}
                data-slot="circular-progress-indeterminate-circle"
              />
            </CircleSVG>
          </div>
        ) : (
          <div
            className="absolute inset-0 animate-spin motion-reduce:animate-none"
            data-slot="circular-progress-indeterminate"
          >
            <CircleSVG
              className="animate-progress-spin"
              data-slot="circular-progress-indeterminate-svg"
            >
              {showTrack && (
                <circle
                  cx="50%"
                  cy="50%"
                  r={radius}
                  strokeWidth={strokeWidth}
                  className={cn(
                    'stroke-surface-container-highest fill-transparent',
                    trackClassName,
                  )}
                  data-slot="circular-progress-track"
                />
              )}

              <circle
                cx="50%"
                cy="50%"
                r={radius}
                strokeWidth={strokeWidth}
                strokeLinecap="round"
                className={cn(
                  'animate-circular-dash fill-transparent stroke-current',
                  'motion-reduce:animate-none',
                  indicatorClassName,
                )}
                data-slot="circular-progress-indeterminate-circle"
              />
            </CircleSVG>
          </div>
        )
      ) : (
        <div
          className={cn('absolute inset-0', variant === 'flat' && '-rotate-90')}
          data-slot="circular-progress-determinate"
        >
          <CircleSVG data-slot="circular-progress-determinate-svg">
            {showTrack && trackArcLength > 0 && (
              <circle
                cx="50%"
                cy="50%"
                r={radius}
                strokeWidth={strokeWidth}
                strokeLinecap="round"
                transform={
                  variant === 'wavy'
                    ? `rotate(-90 ${VIEWBOX_SIZE / 2} ${VIEWBOX_SIZE / 2})`
                    : undefined
                }
                style={{
                  strokeDasharray: `${trackArcLength} ${circumference}`,
                  strokeDashoffset: `${trackDashOffset}`,
                }}
                className={cn(
                  'stroke-surface-container-highest fill-transparent transition-all duration-300 ease-out',
                  trackClassName,
                )}
                data-slot="circular-progress-track"
              />
            )}

            {clampedValue > 0 &&
              (variant === 'wavy' ? (
                <path
                  d={wavyDeterminatePath}
                  strokeWidth={strokeWidth}
                  strokeLinecap="round"
                  className={cn(
                    'fill-transparent stroke-current',
                    indicatorClassName,
                  )}
                  data-slot="circular-progress-determinate-circle"
                />
              ) : (
                <circle
                  cx="50%"
                  cy="50%"
                  r={radius}
                  strokeWidth={strokeWidth}
                  strokeLinecap="round"
                  style={{
                    strokeDasharray: `${activeArcLength} ${circumference}`,
                    strokeDashoffset: '0',
                  }}
                  className={cn(
                    'fill-transparent stroke-current transition-all duration-300 ease-out',
                    indicatorClassName,
                  )}
                  data-slot="circular-progress-determinate-circle"
                />
              ))}
          </CircleSVG>
        </div>
      )}

      {children && (
        <div className="relative z-10 flex items-center justify-center text-xs">
          {children}
        </div>
      )}
    </div>
  )
}

function buildLinearWavePath(height: number, strokeWidth: number) {
  const mid = height / 2
  const amplitude = Math.max(1.5, (height - strokeWidth) / 2 - 0.5)
  const quarter = WAVE_PATTERN_WIDTH / 4
  const half = WAVE_PATTERN_WIDTH / 2

  return [
    `M 0 ${mid}`,
    `Q ${quarter} ${mid - amplitude}, ${half} ${mid}`,
    `T ${WAVE_PATTERN_WIDTH} ${mid}`,
  ].join(' ')
}

function LinearWaveOverlay({
  size,
  className,
}: {
  size: 'sm' | 'md' | 'lg'
  className?: string
}) {
  const rawId = useId()
  const patternId = `linear-wave-${rawId.replaceAll(':', '')}`
  const waveHeight = LINEAR_WAVE_HEIGHT_MAP[size]
  const strokeWidth = LINEAR_STROKE_WIDTH_MAP[size]
  const path = buildLinearWavePath(waveHeight, strokeWidth)

  return (
    <svg
      aria-hidden="true"
      className={cn(
        'text-primary animate-linear-wave h-full shrink-0 motion-reduce:animate-none',
        className,
      )}
      style={{ width: `calc(100% + ${WAVE_PATTERN_WIDTH}px)` }}
    >
      <defs>
        <pattern
          id={patternId}
          width={WAVE_PATTERN_WIDTH}
          height={waveHeight}
          patternUnits="userSpaceOnUse"
        >
          <path
            d={path}
            fill="none"
            stroke="currentColor"
            strokeWidth={strokeWidth}
            strokeLinecap="round"
          />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${patternId})`} />
    </svg>
  )
}

export const linearProgressVariants = cva(
  'relative flex w-full items-center overflow-hidden',
  {
    variants: {
      variant: {
        flat: '',
        wavy: '',
      },
      size: {
        sm: '',
        md: '',
        lg: '',
      },
    },
    compoundVariants: [
      { variant: 'flat', size: 'sm', className: 'h-1' },
      { variant: 'flat', size: 'md', className: 'h-2' },
      { variant: 'flat', size: 'lg', className: 'h-5' },
      { variant: 'wavy', size: 'sm', className: 'h-2.5' },
      { variant: 'wavy', size: 'md', className: 'h-4' },
      { variant: 'wavy', size: 'lg', className: 'h-5' },
    ],
    defaultVariants: {
      variant: 'flat',
      size: 'sm',
    },
  },
)

const linearTrackVariants = cva('bg-surface-container-highest relative', {
  variants: {
    variant: {
      flat: 'h-full',
      wavy: 'rounded-full',
    },
    size: {
      sm: '',
      md: '',
      lg: '',
    },
  },
  compoundVariants: [
    { variant: 'wavy', size: 'sm', className: 'h-1' },
    { variant: 'wavy', size: 'md', className: 'h-2' },
    { variant: 'wavy', size: 'lg', className: 'h-3' },
  ],
  defaultVariants: {
    variant: 'flat',
    size: 'sm',
  },
})

export type LinearProgressProps = ComponentProps<'div'> &
  VariantProps<typeof linearProgressVariants> & {
    indeterminate?: boolean
    value?: number
    variant?: 'flat' | 'wavy'
    size?: 'sm' | 'md' | 'lg'
    stopIndicator?: boolean
    trackClassName?: string
    indicatorClassName?: string
  }

export function LinearProgress({
  value,
  indeterminate = false,
  variant = 'flat',
  size = 'sm',
  stopIndicator = true,
  className,
  trackClassName,
  indicatorClassName,
  ...props
}: LinearProgressProps) {
  const clampedValue = clampProgress(value, 0)
  const isHero = variant === 'flat' && size === 'lg'
  const showActive = indeterminate || clampedValue > 0
  const showTrack = indeterminate || clampedValue < 100
  const showGap = !indeterminate && clampedValue > 0 && clampedValue < 100
  const showStop = !indeterminate && stopIndicator && clampedValue < 99

  return (
    <div
      className={cn(
        linearProgressVariants({ variant, size }),
        showGap && 'gap-1',
        className,
      )}
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : clampedValue}
      aria-valuemin={0}
      aria-valuemax={100}
      data-slot="linear-progress"
      data-variant={variant}
      data-size={size}
      data-indeterminate={indeterminate ? 'true' : undefined}
      {...props}
    >
      {indeterminate ? (
        <>
          <div
            className={cn(
              linearTrackVariants({ variant, size }),
              'w-full',
              isHero ? 'rounded-small' : 'rounded-full',
              trackClassName,
            )}
            data-slot="linear-progress-track"
          />

          <div
            className={cn(
              'animate-linear-progress-primary absolute inset-y-0 left-0 overflow-hidden',
              variant === 'flat' && 'bg-primary',
              isHero ? 'rounded-small' : 'rounded-full',
              indicatorClassName,
            )}
            data-slot="linear-progress-indeterminate-primary"
          >
            {variant === 'wavy' && <LinearWaveOverlay size={size} />}
          </div>

          <div
            className={cn(
              'animate-linear-progress-secondary absolute inset-y-0 left-0 overflow-hidden',
              variant === 'flat' && 'bg-primary',
              isHero ? 'rounded-small' : 'rounded-full',
              indicatorClassName,
            )}
            data-slot="linear-progress-indeterminate-secondary"
          >
            {variant === 'wavy' && <LinearWaveOverlay size={size} />}
          </div>
        </>
      ) : (
        <>
          {showActive && (
            <div
              className={cn(
                'relative h-full shrink-0 overflow-hidden transition-[width] duration-300 ease-out motion-reduce:transition-none',
                variant === 'flat' && 'bg-primary',
                isHero
                  ? showTrack
                    ? 'rounded-l-small rounded-r-extra-small'
                    : 'rounded-small'
                  : 'rounded-full',
                indicatorClassName,
              )}
              style={{
                width: showTrack ? `calc(${clampedValue}% - 2px)` : '100%',
              }}
              data-slot="linear-progress-indicator"
            >
              {variant === 'wavy' && <LinearWaveOverlay size={size} />}
            </div>
          )}

          {showTrack && (
            <div
              className={cn(
                linearTrackVariants({ variant, size }),
                'min-w-0 flex-1',
                isHero
                  ? showActive
                    ? 'rounded-l-extra-small rounded-r-small'
                    : 'rounded-small'
                  : 'rounded-full',
                trackClassName,
              )}
              data-slot="linear-progress-track"
            >
              {showStop && (
                <span
                  aria-hidden="true"
                  data-slot="linear-progress-stop"
                  className={cn(
                    'bg-primary absolute top-1/2 -translate-y-1/2 rounded-full',
                    isHero ? 'right-1.5 size-1' : 'right-0 size-1',
                  )}
                />
              )}
            </div>
          )}
        </>
      )}
    </div>
  )
}
