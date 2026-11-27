import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/utils'
import { Circle, CircleSVG } from './circle'

const HalfCircle = (props: Omit<ComponentProps<typeof Circle>, 'value'>) => {
  return <Circle value={50} {...props} />
}

const HalfCircleSVG = ({
  className,
  ...props
}: ComponentProps<typeof CircleSVG>) => {
  return <CircleSVG className={cn('w-[200%]', className)} {...props} />
}

const HalfCircleContainer = ({
  className,
  ...props
}: ComponentProps<'div'>) => {
  return (
    <div
      className={cn(
        'relative h-full w-1/2 shrink-0 overflow-hidden',
        className,
      )}
      {...props}
    />
  )
}

export function CircularProgress({
  value,
  indeterminate,
  className,
  children,
  ...props
}: ComponentProps<'div'> & {
  indeterminate?: boolean
  value?: number
}) {
  return (
    <div
      className={cn('relative size-12 overflow-hidden', className)}
      data-slot="circular-progress"
      {...props}
    >
      {indeterminate ? (
        <div
          className="absolute inset-0 animate-spin"
          data-slot="circular-progress-indeterminate"
        >
          <div
            className="animate-progress-spin absolute inset-0 flex"
            data-slot="circular-progress-indeterminate-inner"
          >
            {/* left */}
            <HalfCircleContainer data-slot="circular-progress-indeterminate-left">
              <HalfCircleSVG className="animate-progress-spin-left">
                <HalfCircle data-slot="circular-progress-indeterminate-left-circle" />
              </HalfCircleSVG>
            </HalfCircleContainer>

            {/* right */}
            <HalfCircleContainer data-slot="circular-progress-indeterminate-right">
              <HalfCircleSVG className="animate-progress-spin-right -left-full">
                <HalfCircle data-slot="circular-progress-indeterminate-right-circle" />
              </HalfCircleSVG>
            </HalfCircleContainer>
          </div>
        </div>
      ) : (
        <div
          className="absolute h-full w-full -rotate-90"
          data-slot="circular-progress-determinate"
        >
          <CircleSVG data-slot="circular-progress-determinate-svg">
            <Circle
              data-slot="circular-progress-determinate-circle"
              className="transition-all"
              value={value ?? 100}
            />
          </CircleSVG>
        </div>
      )}

      {children && (
        <div className="absolute inset-0 flex items-center justify-center text-xs">
          {children}
        </div>
      )}
    </div>
  )
}

const LINEAR_PROGRESS_HEIGHT = {
  sm: 'h-1',
  md: 'h-2',
  lg: 'h-5',
} as const

export function LinearProgress({
  value,
  indeterminate,
  size = 'sm',
  className,
  ...props
}: ComponentProps<'div'> & {
  indeterminate?: boolean
  value?: number
  size?: keyof typeof LINEAR_PROGRESS_HEIGHT
}) {
  const clampedValue = Math.min(100, Math.max(0, value ?? 0))
  const isHero = size === 'lg'
  const showActive = clampedValue > 0
  const showTrack = clampedValue < 100

  return (
    <div
      className={cn(
        'relative flex w-full items-center overflow-hidden',
        LINEAR_PROGRESS_HEIGHT[size],
        indeterminate
          ? cn(
              'bg-surface-container-highest',
              isHero ? 'rounded-small' : 'rounded-full',
            )
          : showActive && showTrack && 'gap-1',
        className,
      )}
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : clampedValue}
      aria-valuemin={0}
      aria-valuemax={100}
      data-slot="linear-progress"
      data-size={size}
      {...props}
    >
      {indeterminate ? (
        <>
          {/* Primary indicator - moves from left to right */}
          <div
            className={cn(
              'animate-linear-progress-primary bg-primary absolute inset-y-0 left-0',
              isHero ? 'rounded-small' : 'rounded-full',
            )}
            data-slot="linear-progress-indeterminate-primary"
          />

          {/* Secondary indicator - follows with different timing */}
          <div
            className={cn(
              'animate-linear-progress-secondary bg-primary absolute inset-y-0 left-0',
              isHero ? 'rounded-small' : 'rounded-full',
            )}
            data-slot="linear-progress-indeterminate-secondary"
          />
        </>
      ) : (
        <>
          {showActive && (
            <div
              className={cn(
                'bg-primary h-full shrink-0 transition-[width] duration-300 ease-out motion-reduce:transition-none',
                isHero
                  ? showTrack
                    ? 'rounded-l-small rounded-r-extra-small'
                    : 'rounded-small'
                  : 'rounded-full',
              )}
              style={{
                width: showTrack ? `calc(${clampedValue}% - 2px)` : '100%',
              }}
              data-slot="linear-progress-indicator"
            />
          )}

          {showTrack && (
            <div
              className={cn(
                'bg-surface-container-highest relative h-full min-w-0 flex-1',
                isHero
                  ? showActive
                    ? 'rounded-l-extra-small rounded-r-small'
                    : 'rounded-small'
                  : 'rounded-full',
              )}
              data-slot="linear-progress-track"
            >
              {clampedValue < 99 && (
                <span
                  aria-hidden
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
