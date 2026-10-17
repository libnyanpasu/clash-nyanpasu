import { clamp } from 'motion/react'
import { ComponentProps, useRef } from 'react'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'

const THUMB_TARGET_SIZE_PX = 48
const THUMB_GAP_OFFSET_PX = 8

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
  onPointerDown,
  onPointerUp,
  onPointerCancel,
  onKeyUp,
  onBlur,
  ...props
}: Omit<
  ComponentProps<'input'>,
  'type' | 'value' | 'defaultValue' | 'min' | 'max' | 'onChange'
> & {
  value?: number
  defaultValue?: number
  min?: number
  max?: number
  onValueChange?: (value: number) => void
  onValueCommit?: (value: number) => void
}) {
  const controlledValue =
    typeof value === 'number' ? clamp(min, max, value) : undefined

  const defaultSliderValue = clamp(min, max, defaultValue ?? min)

  const [rawValue, setRawValue] = useControllableState<number>({
    prop: controlledValue,
    defaultProp: defaultSliderValue,
    onChange: (nextValue) => {
      onValueChange?.(clamp(min, max, nextValue))
    },
  })

  const currentValue = clamp(min, max, rawValue ?? min)
  const pendingValue = useRef<number | null>(null)

  const percentage =
    max === min ? 0 : ((currentValue - min) / (max - min)) * 100

  const ratio = percentage / 100

  // Match the invisible native thumb's travel to the visible handle exactly.
  const thumbOffsetPx = THUMB_TARGET_SIZE_PX / 2
  const thumbLeft = `calc(${thumbOffsetPx}px + (100% - ${thumbOffsetPx * 2}px) * ${ratio})`
  const rangeWidth = `calc(${thumbLeft} - ${THUMB_GAP_OFFSET_PX}px)`
  const trackWidth = `calc(100% - ${thumbLeft} - ${THUMB_GAP_OFFSET_PX}px)`

  const handleValueChange: ComponentProps<'input'>['onChange'] = (event) => {
    const nextValue = clamp(min, max, Number(event.target.value))
    pendingValue.current = nextValue
    setRawValue(nextValue)
  }

  const commitValue = () => {
    if (pendingValue.current === null) return

    const nextValue = pendingValue.current
    pendingValue.current = null
    onValueCommit?.(nextValue)
  }

  return (
    <div
      data-slot="slider"
      data-disabled={String(disabled)}
      className={cn(
        'relative flex h-12 w-full touch-none items-center select-none',
        'has-focus-visible:ring-primary rounded-full has-focus-visible:ring-2 has-focus-visible:ring-offset-2 data-[disabled=true]:opacity-50',
        className,
      )}
    >
      <span
        data-slot="slider-range"
        className="bg-primary pointer-events-none absolute left-0 h-4 rounded-l-full rounded-r-xs"
        style={{
          width: rangeWidth,
        }}
      />

      <span
        data-slot="slider-track"
        className="bg-secondary-container pointer-events-none absolute right-0 h-4 rounded-l-xs rounded-r-full"
        style={{
          width: trackWidth,
        }}
      />

      <span
        data-slot="slider-thumb"
        className="bg-primary pointer-events-none absolute top-1/2 h-11 w-1 -translate-x-1/2 -translate-y-1/2 rounded-full"
        style={{
          left: thumbLeft,
        }}
      />

      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={currentValue}
        disabled={disabled}
        onChange={handleValueChange}
        onPointerDown={(event) => {
          onPointerDown?.(event)
          if (!event.defaultPrevented && !disabled) {
            event.currentTarget.setPointerCapture(event.pointerId)
          }
        }}
        onPointerUp={(event) => {
          commitValue()
          onPointerUp?.(event)
        }}
        onPointerCancel={(event) => {
          commitValue()
          onPointerCancel?.(event)
        }}
        onKeyUp={(event) => {
          commitValue()
          onKeyUp?.(event)
        }}
        onBlur={(event) => {
          commitValue()
          onBlur?.(event)
        }}
        className="absolute inset-0 m-0 h-full w-full cursor-pointer touch-none appearance-none bg-transparent opacity-0 disabled:cursor-not-allowed [&::-moz-range-thumb]:size-12 [&::-moz-range-thumb]:border-0 [&::-webkit-slider-thumb]:size-12 [&::-webkit-slider-thumb]:appearance-none"
        {...props}
      />
    </div>
  )
}
