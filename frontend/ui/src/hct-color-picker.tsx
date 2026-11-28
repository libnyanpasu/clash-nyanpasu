import Check from '~icons/material-symbols/check-rounded'
import { cva } from 'class-variance-authority'
import { Slider as SliderPrimitive } from 'radix-ui'
import {
  ComponentProps,
  createContext,
  MouseEvent,
  useContext,
  useMemo,
  useState,
} from 'react'
import {
  argbFromHex,
  Hct,
  hexFromArgb,
} from '@material/material-color-utilities'
import { cn } from '@nyanpasu/utils'
import { useControllableState } from '@radix-ui/react-use-controllable-state'
import { Input } from './input'

export type HctChannel = 'hue' | 'chroma' | 'tone'

type HctValue = Record<HctChannel, number>

const CHANNEL_MAX = {
  hue: 360,
  chroma: 150,
  tone: 100,
} as const satisfies Record<HctChannel, number>

// sRGB grays are not chroma 0 in HCT: they reach about 2.87 (#ffffff). Below
// this their hue is noise, so a gray keeps the hue the user chose.
const ACHROMATIC_CHROMA = 3

const GRADIENT_STOPS = 100

// The handle is this wide, so its center travels from half of it to the
// width minus half of it.
const HANDLE_WIDTH_PX = 4

const FALLBACK_HEX = '#000000'

const HEX_PATTERN = /^(?:[0-9a-f]{3}|[0-9a-f]{6})$/i

/**
 * Parses `#rgb` or `#rrggbb` (the `#` is optional) into lowercase `#rrggbb`.
 * Eight digits are rejected: Material Color Utilities reads them as ARGB,
 * not CSS's RRGGBBAA.
 */
export function parseHexColor(input: string): string | null {
  const hex = input.trim().replace(/^#/, '')

  if (!HEX_PATTERN.test(hex)) {
    return null
  }

  const full =
    hex.length === 3 ? [...hex].map((digit) => digit + digit).join('') : hex

  return `#${full.toLowerCase()}`
}

const hexFromHct = ({ hue, chroma, tone }: HctValue) =>
  hexFromArgb(Hct.from(hue, chroma, tone).toInt())

const hctFromHex = (hex: string, previous?: HctValue): HctValue => {
  const hct = Hct.fromInt(argbFromHex(hex))

  return {
    hue: previous && hct.chroma < ACHROMATIC_CHROMA ? previous.hue : hct.hue,
    chroma: hct.chroma,
    tone: hct.tone,
  }
}

const buildGradient = (colorAt: (ratio: number) => string) => {
  const stops = Array.from(
    { length: GRADIENT_STOPS },
    (_, index) =>
      `${colorAt(index / GRADIENT_STOPS)} calc(${HANDLE_WIDTH_PX / 2}px + (100% - ${HANDLE_WIDTH_PX}px) * ${index / GRADIENT_STOPS})`,
  )

  return `linear-gradient(to right, ${stops.join(', ')})`
}

// Mirrors material-web's catalog hct-slider: a saturated mid-tone hue
// strip, the current hue's chroma at mid tone, and a gray tone ramp.
const buildChannelGradient = (channel: HctChannel, hue: number) => {
  switch (channel) {
    case 'hue':
      return buildGradient((ratio) =>
        hexFromHct({ hue: CHANNEL_MAX.hue * ratio, chroma: 100, tone: 50 }),
      )
    case 'chroma':
      return buildGradient((ratio) =>
        hexFromHct({ hue, chroma: CHANNEL_MAX.chroma * ratio, tone: 50 }),
      )
    case 'tone':
      return buildGradient((ratio) =>
        hexFromHct({ hue: 0, chroma: 0, tone: CHANNEL_MAX.tone * ratio }),
      )
  }
}

const presetVariants = cva([
  'grid size-11 shrink-0 cursor-pointer place-items-center',
  'border-outline-variant/40 border',
  'outline-primary outline-offset-2',
  'transition-[border-radius,outline-offset,outline-width] duration-200 ease-[cubic-bezier(0.2,0,0,1)]',
  'focus-visible:outline-[3px] motion-reduce:transition-none',
  'data-[state=checked]:rounded-medium data-[state=checked]:outline-2',
  'data-[state=unchecked]:rounded-exl data-[state=unchecked]:hover:rounded-large',
])

const slidersVariants = cva([
  'bg-surface-container text-on-surface-variant rounded-extra-large',
  'flex flex-col gap-5 px-5 py-4',
])

const sliderTrackVariants = cva([
  'rounded-full relative h-5 grow overflow-hidden',
  'shadow-[inset_0_0_0_1px_var(--color-outline-variant)]',
])

const sliderThumbVariants = cva([
  'group relative block h-11 cursor-pointer outline-hidden',
  'before:absolute before:-inset-x-3 before:inset-y-0 before:content-[""]',
])

const sliderHandleVariants = cva([
  'rounded-full bg-on-surface ring-surface absolute inset-0 ring-2',
  'origin-center transition-[transform,background-color,box-shadow] duration-150 ease-[cubic-bezier(0.2,0,0,1)]',
  'group-hover:bg-primary group-focus-visible:bg-primary group-focus-visible:ring-primary/40 group-focus-visible:ring-4',
  'group-active:scale-x-50 motion-reduce:transition-none',
])

const sliderBubbleVariants = cva([
  'bg-inverse-surface text-inverse-on-surface rounded-full',
  'absolute bottom-[calc(100%+4px)] left-1/2 grid min-h-7 min-w-9 -translate-x-1/2 place-items-center px-2',
  'text-xs leading-4 font-medium tabular-nums',
  'origin-bottom scale-0 transition-transform duration-100 ease-[cubic-bezier(0.2,0,0,1)] motion-reduce:transition-none',
  'group-hover:scale-100 group-focus-visible:scale-100 group-active:scale-100',
])

type HctColorPickerContextValue = {
  hex: string
  hct: HctValue
  setChannel: (channel: HctChannel, value: number) => void
  setHex: (hex: string) => void
}

const HctColorPickerContext = createContext<HctColorPickerContextValue | null>(
  null,
)

const useHctColorPickerContext = () => {
  const context = useContext(HctColorPickerContext)

  if (context === null) {
    throw new Error(
      'HctColorPicker parts must be rendered inside HctColorPicker',
    )
  }

  return context
}

export function HctColorPicker({
  value,
  defaultValue,
  onValueChange,
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'defaultValue' | 'onChange'> & {
  value?: string
  defaultValue?: string
  onValueChange?: (hex: string) => void
}) {
  const [hex, setHexValue] = useControllableState({
    prop:
      value === undefined ? undefined : (parseHexColor(value) ?? FALLBACK_HEX),
    defaultProp: parseHexColor(defaultValue ?? '') ?? FALLBACK_HEX,
    onChange: onValueChange,
  })

  // HCT is wider than sRGB and a gray has no hue, so the channels cannot be
  // recovered from the hex; keep them, and follow the hex only when it
  // changes from outside.
  const [state, setState] = useState(() => ({ hex, hct: hctFromHex(hex) }))

  const hct = state.hex === hex ? state.hct : hctFromHex(hex, state.hct)

  if (state.hex !== hex) {
    setState({ hex, hct })
  }

  const setChannel = (channel: HctChannel, channelValue: number) => {
    const nextHct = { ...hct, [channel]: channelValue }
    const nextHex = hexFromHct(nextHct)

    setState({ hex: nextHex, hct: nextHct })
    setHexValue(nextHex)
  }

  const setHex = (nextHex: string) => {
    setState({ hex: nextHex, hct: hctFromHex(nextHex, hct) })
    setHexValue(nextHex)
  }

  return (
    <HctColorPickerContext.Provider value={{ hex, hct, setChannel, setHex }}>
      <div
        data-slot="hct-color-picker"
        className={cn('flex flex-col gap-4', className)}
        {...props}
      />
    </HctColorPickerContext.Provider>
  )
}

export function HctColorPickerPresets({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      role="radiogroup"
      data-slot="hct-color-picker-presets"
      className={cn('flex flex-wrap items-center gap-3 p-1', className)}
      {...props}
    />
  )
}

export function HctColorPickerPreset({
  value,
  label,
  className,
  style,
  onClick,
  ...props
}: Omit<ComponentProps<'button'>, 'value' | 'children'> & {
  value: string
  label?: string
}) {
  const { hex, setHex } = useHctColorPickerContext()

  const color = parseHexColor(value)

  if (color === null) {
    return null
  }

  const checked = color === hex
  const name = label ?? color
  const isLight = Hct.fromInt(argbFromHex(color)).tone > 60

  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    onClick?.(event)

    if (!event.defaultPrevented) {
      setHex(color)
    }
  }

  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-label={name}
      title={name}
      data-slot="hct-color-picker-preset"
      data-state={checked ? 'checked' : 'unchecked'}
      className={cn(
        presetVariants(),
        isLight ? 'text-black' : 'text-white',
        className,
      )}
      style={{ ...style, backgroundColor: color }}
      onClick={handleClick}
      {...props}
    >
      {checked && (
        <Check data-slot="hct-color-picker-preset-check" className="size-6" />
      )}
    </button>
  )
}

export function HctColorPickerHexField({
  label,
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'> & { label: string }) {
  const { hex, setHex } = useHctColorPickerContext()

  // The text being edited; null while the field shows the current color.
  const [text, setText] = useState<string | null>(null)

  const invalid = text !== null && parseHexColor(text) === null

  return (
    <div
      data-slot="hct-color-picker-hex-field"
      className={cn('flex', className)}
      {...props}
    >
      <Input
        label={label}
        // `Input` renders its floating label without `htmlFor`.
        aria-label={label}
        data-slot="hct-color-picker-hex-input"
        className="font-mono"
        value={text ?? hex}
        aria-invalid={invalid || undefined}
        onFocus={() => setText(hex)}
        onChange={(event) => {
          setText(event.target.value)

          const parsed = parseHexColor(event.target.value)

          if (parsed !== null) {
            setHex(parsed)
          }
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            setText(hex)
          }
        }}
        onBlur={() => setText(null)}
      >
        <span
          aria-hidden
          data-slot="hct-color-picker-swatch"
          className="border-outline-variant size-6 shrink-0 rounded-full border"
          style={{ backgroundColor: hex }}
        />
      </Input>
    </div>
  )
}

export function HctColorPickerSliders({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      data-slot="hct-color-picker-sliders"
      className={cn(slidersVariants(), className)}
      {...props}
    />
  )
}

export function HctColorPickerSlider({
  channel,
  label,
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'> & {
  channel: HctChannel
  label: string
}) {
  const { hct, setChannel } = useHctColorPickerContext()

  // Only the chroma strip depends on the color (its hue).
  const gradientHue = channel === 'chroma' ? hct.hue : 0

  const gradient = useMemo(
    () => buildChannelGradient(channel, gradientHue),
    [channel, gradientHue],
  )

  const current = Math.round(hct[channel])

  return (
    <div
      data-slot="hct-color-picker-slider"
      className={cn('flex flex-col gap-1', className)}
      {...props}
    >
      <div className="flex items-baseline justify-between">
        <span
          data-slot="hct-color-picker-slider-label"
          className="text-sm font-medium"
        >
          {label}
        </span>

        <span
          aria-hidden
          data-slot="hct-color-picker-slider-readout"
          className="text-on-surface font-mono text-sm tabular-nums"
        >
          {current}
        </span>
      </div>

      {/* The gradient is the track. The handle center travels from half its width to width minus half, and the gradient stops are inset by the same amount, so the handle sits on the color of its value. */}
      <SliderPrimitive.Root
        data-slot="hct-color-picker-slider-root"
        className="relative flex h-11 w-full touch-none items-center select-none"
        value={[hct[channel]]}
        min={0}
        max={CHANNEL_MAX[channel]}
        step={1}
        onValueChange={([next]) => setChannel(channel, next)}
      >
        <SliderPrimitive.Track
          data-slot="hct-color-picker-slider-track"
          className={sliderTrackVariants()}
        >
          <span
            aria-hidden
            data-slot="hct-color-picker-gradient"
            className="absolute inset-0"
            style={{ background: gradient }}
          />
        </SliderPrimitive.Track>

        <SliderPrimitive.Thumb
          aria-label={label}
          data-slot="hct-color-picker-slider-thumb"
          className={sliderThumbVariants()}
          style={{ width: HANDLE_WIDTH_PX }}
        >
          <span
            aria-hidden
            data-slot="hct-color-picker-slider-handle"
            className={sliderHandleVariants()}
          />

          <span
            aria-hidden
            data-slot="hct-color-picker-slider-value"
            className={sliderBubbleVariants()}
          >
            {current}
          </span>
        </SliderPrimitive.Thumb>
      </SliderPrimitive.Root>
    </div>
  )
}
