import Check from '~icons/material-symbols/check-rounded'
import { Slider as SliderPrimitive } from 'radix-ui'
import {
  ComponentProps,
  createContext,
  MouseEvent,
  useContext,
  useId,
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
    (_, index) => `${colorAt(index / GRADIENT_STOPS)} ${index}%`,
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
      className={cn('flex flex-wrap items-center gap-2', className)}
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
        'border-outline-variant grid size-8 cursor-pointer place-items-center rounded-full border',
        'focus-visible:outline-primary outline-offset-2 focus-visible:outline-2',
        isLight ? 'text-black' : 'text-white',
        className,
      )}
      style={{ ...style, backgroundColor: color }}
      onClick={handleClick}
      {...props}
    >
      {checked && <Check className="size-5" />}
    </button>
  )
}

export function HctColorPickerHexField({
  label,
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'> & { label: string }) {
  const { hex, setHex } = useHctColorPickerContext()

  const inputId = useId()

  // The text being edited; null while the field shows the current color.
  const [text, setText] = useState<string | null>(null)

  const invalid = text !== null && parseHexColor(text) === null

  return (
    <div
      data-slot="hct-color-picker-hex-field"
      className={cn(
        'bg-surface-variant text-on-surface-variant flex items-center gap-3 rounded-3xl py-3 pr-3 pl-5',
        className,
      )}
      {...props}
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <label
          data-slot="hct-color-picker-hex-label"
          htmlFor={inputId}
          className="text-sm"
        >
          {label}
        </label>

        <input
          id={inputId}
          data-slot="hct-color-picker-hex-input"
          className={cn(
            'w-full border-b-2 border-transparent bg-transparent font-mono text-base outline-hidden',
            'focus:border-primary',
            'aria-[invalid=true]:text-error aria-[invalid=true]:focus:border-error',
          )}
          value={text ?? hex}
          aria-invalid={invalid || undefined}
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
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
        />
      </div>

      <span
        aria-hidden
        data-slot="hct-color-picker-swatch"
        className="border-outline size-12 shrink-0 rounded-full border"
        style={{ backgroundColor: hex }}
      />
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
      className={cn(
        'bg-surface-variant text-on-surface-variant flex flex-col gap-4 rounded-3xl px-5 py-4',
        className,
      )}
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

  return (
    <div
      data-slot="hct-color-picker-slider"
      className={cn('flex flex-col gap-1', className)}
      {...props}
    >
      {/* The thumb center travels from 10px to width - 10px, so the label, track and gradient are inset mx-2.5 to line the thumb up with the gradient color of its value. */}
      <span
        data-slot="hct-color-picker-slider-label"
        className="mx-2.5 text-sm"
      >
        {label}
      </span>

      <SliderPrimitive.Root
        data-slot="hct-color-picker-slider-root"
        className="relative flex h-10 w-full touch-none items-center select-none"
        value={[hct[channel]]}
        min={0}
        max={CHANNEL_MAX[channel]}
        step={1}
        onValueChange={([next]) => setChannel(channel, next)}
      >
        <SliderPrimitive.Track
          data-slot="hct-color-picker-slider-track"
          className="bg-on-surface-variant relative mx-2.5 h-1 grow rounded-full"
        >
          <SliderPrimitive.Range
            data-slot="hct-color-picker-slider-range"
            className="bg-primary absolute h-full rounded-full"
          />
        </SliderPrimitive.Track>

        <SliderPrimitive.Thumb
          aria-label={label}
          data-slot="hct-color-picker-slider-thumb"
          className={cn(
            'group bg-primary relative block size-5 cursor-pointer rounded-full shadow-sm outline-hidden',
            'before:bg-primary before:absolute before:-inset-2.5 before:rounded-full before:opacity-0 before:transition-opacity',
            'hover:before:opacity-[0.08] focus:before:opacity-[0.12] active:before:opacity-[0.12]',
          )}
        >
          <span
            aria-hidden
            data-slot="hct-color-picker-slider-value"
            className={cn(
              'bg-primary text-on-primary absolute bottom-[calc(100%+10px)] left-1/2 grid min-h-7 min-w-7 origin-bottom -translate-x-1/2 scale-0 place-items-center rounded-full p-1 text-xs leading-4 font-medium tabular-nums',
              'transition-transform duration-100 ease-[cubic-bezier(0.2,0,0,1)]',
              'before:bg-primary before:absolute before:-bottom-[2.8px] before:left-1/2 before:size-3.5 before:-translate-x-1/2 before:rotate-45',
              'after:bg-primary after:absolute after:inset-0 after:rounded-full',
              'group-hover:scale-100 group-focus:scale-100 group-active:scale-100',
            )}
          >
            <span className="relative z-10">{Math.round(hct[channel])}</span>
          </span>
        </SliderPrimitive.Thumb>
      </SliderPrimitive.Root>

      <div
        aria-hidden
        data-slot="hct-color-picker-gradient"
        className="mx-2.5 h-6 rounded-full border border-current"
        style={{ background: gradient }}
      />
    </div>
  )
}
