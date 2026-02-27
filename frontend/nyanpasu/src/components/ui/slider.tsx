import { clamp, motion, Transition } from 'framer-motion'
import { ComponentProps } from 'react'
import { cn } from '@nyanpasu/ui'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const EDGE_OFFSET_PX = 16
const PADDING_PX = 8

export function Slider({
  className,
  defaultValue,
  value,
  min = 0,
  max = 100,
  disabled,
  step = 1,
  onValueChange,
  onValueCommit,
  onMouseUp,
  onTouchEnd,
  onKeyUp,
  onBlur,
  ...props
}: Omit<
  ComponentProps<'input'>,
  'type' | 'value' | 'defaultValue' | 'min' | 'max' | 'onChange'
> & {
  value?: number[]
  defaultValue?: number[]
  min?: number
  max?: number
  onValueChange?: (value: number[]) => void
  onValueCommit?: (value: number[]) => void
}) {
  const controlledValue = Array.isArray(value)
    ? clamp(min, max, value[0] ?? min)
    : undefined

  const defaultSliderValue = clamp(
    min,
    max,
    Array.isArray(defaultValue) ? (defaultValue[0] ?? min) : min,
  )

  const [rawValue, setRawValue] = useControllableState<number>({
    prop: controlledValue,
    defaultProp: defaultSliderValue,
    onChange: (nextValue) => {
      onValueChange?.([clamp(min, max, nextValue)])
    },
  })

  const currentValue = clamp(min, max, rawValue ?? min)

  const percentage =
    max === min ? 0 : ((currentValue - min) / (max - min)) * 100

  const ratio = percentage / 100

  const thumbOffsetPx = EDGE_OFFSET_PX + PADDING_PX
  const thumbLeft = `calc(${thumbOffsetPx}px + (100% - ${thumbOffsetPx * 2}px) * ${ratio})`
  const rangeWidth = `calc(${thumbLeft} - ${PADDING_PX}px)`
  const trackWidth = `calc(100% - ${thumbLeft} - ${PADDING_PX}px)`

  const motionTransition: Transition = disabled
    ? { duration: 0 }
    : { type: 'spring' as const, stiffness: 380, damping: 35, mass: 0.2 }

  const handleValueChange: ComponentProps<'input'>['onChange'] = (event) => {
    const nextValue = clamp(min, max, Number(event.target.value))
    setRawValue(nextValue)
  }

  const commitValue = () => {
    onValueCommit?.([currentValue])
  }

  return (
    <div
      data-slot="slider"
      data-disabled={String(disabled)}
      className={cn(
        'relative flex w-full touch-none items-center justify-between select-none',
        'h-4 data-[disabled=true]:opacity-50',
        className,
      )}
    >
      <motion.span
        data-slot="slider-range"
        className={cn(
          'bg-primary absolute inset-y-0 left-0 select-none',
          'rounded-l-full rounded-r-sm',
        )}
        animate={{
          width: rangeWidth,
          borderRadius: '12px 4px 4px 12px',
        }}
        transition={motionTransition}
      />

      <motion.span
        data-slot="slider-track"
        className={cn('bg-surface absolute inset-y-0 right-0 select-none')}
        animate={{
          width: trackWidth,
          borderRadius: '4px 12px 12px 4px',
        }}
        transition={motionTransition}
      />

      <motion.span
        data-slot="slider-thumb"
        className={cn(
          'bg-primary pointer-events-none absolute top-1/2 h-10 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full',
          'transition-[color,box-shadow] select-none',
        )}
        animate={{
          left: thumbLeft,
        }}
        transition={motionTransition}
      />

      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={currentValue}
        disabled={disabled}
        onChange={handleValueChange}
        onMouseUp={(event) => {
          commitValue()
          onMouseUp?.(event)
        }}
        onTouchEnd={(event) => {
          commitValue()
          onTouchEnd?.(event)
        }}
        onKeyUp={(event) => {
          commitValue()
          onKeyUp?.(event)
        }}
        onBlur={(event) => {
          commitValue()
          onBlur?.(event)
        }}
        className="absolute inset-0 h-full w-full cursor-pointer appearance-none bg-transparent opacity-0 disabled:cursor-not-allowed"
        {...props}
      />
    </div>
  )
}
