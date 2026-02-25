import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'
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

export function LinearProgress({
  value,
  indeterminate,
  className,
  ...props
}: ComponentProps<'div'> & {
  indeterminate?: boolean
  value?: number
}) {
  const clampedValue = Math.min(100, Math.max(0, value ?? 0))

  return (
    <div
      className={cn(
        'bg-secondary-container relative h-3 w-full overflow-hidden rounded-full',
        className,
      )}
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : clampedValue}
      aria-valuemin={0}
      aria-valuemax={100}
      data-slot="linear-progress"
      {...props}
    >
      {indeterminate ? (
        <>
          {/* Primary indicator - moves from left to right */}
          <div
            className="animate-linear-progress-primary bg-primary absolute inset-y-0 left-0 rounded-full"
            data-slot="linear-progress-indeterminate-primary"
          />

          {/* Secondary indicator - follows with different timing */}
          <div
            className="animate-linear-progress-secondary bg-primary absolute inset-y-0 left-0 rounded-full"
            data-slot="linear-progress-indeterminate-secondary"
          />
        </>
      ) : (
        <div
          className="bg-primary absolute inset-y-0 left-0 rounded-full transition-[width] duration-300 ease-out"
          style={{
            width: `${clampedValue}%`,
          }}
          data-slot="linear-progress-indicator"
        />
      )}
    </div>
  )
}
