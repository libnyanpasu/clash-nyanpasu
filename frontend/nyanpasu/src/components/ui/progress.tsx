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
        'relative inline-flex h-full w-1/2 overflow-hidden',
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
          className="absolute h-full w-full animate-spin"
          data-slot="circular-progress-indeterminate"
        >
          <div
            className="animate-progress-spin absolute h-full w-full"
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
