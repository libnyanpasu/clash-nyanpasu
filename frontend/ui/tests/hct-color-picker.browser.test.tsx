import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import {
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerPreset,
  HctColorPickerPresets,
  HctColorPickerSlider,
  HctColorPickerSliders,
  parseHexColor,
} from '@nyanpasu/ui/hct-color-picker'
import {
  argbFromHex,
  Hct,
  hexFromArgb,
} from '@material/material-color-utilities'

const hctOf = (hex: string) => Hct.fromInt(argbFromHex(hex))

const hexOf = (hue: number, chroma: number, tone: number) =>
  hexFromArgb(Hct.from(hue, chroma, tone).toInt())

const PAGE = '{PageUp}'

// Radix moves a slider by keyboard: Home/End jump to the ends, PageUp is
// ten steps, arrows one.
const press = async (thumb: HTMLElement, keys: string) => {
  thumb.focus()
  await userEvent.keyboard(keys)
}

const controls = { setValue: (_hex: string) => {} }

function Picker({
  initial,
  onValueChange,
}: {
  initial: string
  onValueChange: (hex: string) => void
}) {
  const [value, setValue] = useState(initial)
  controls.setValue = setValue

  return (
    <HctColorPicker
      value={value}
      onValueChange={(hex) => {
        setValue(hex)
        onValueChange(hex)
      }}
    >
      <HctColorPickerPresets aria-label="Presets">
        <HctColorPickerPreset value="#9e1e67" />
        <HctColorPickerPreset value="#1867C0" label="Accent" />
        <HctColorPickerPreset value="not a color" label="Broken" />
      </HctColorPickerPresets>

      <HctColorPickerHexField label="Hex" />

      <HctColorPickerSliders>
        <HctColorPickerSlider channel="hue" label="Hue" />
        <HctColorPickerSlider channel="chroma" label="Chroma" />
        <HctColorPickerSlider channel="tone" label="Tone" />
      </HctColorPickerSliders>
    </HctColorPicker>
  )
}

async function mount(
  onTestFinished: (fn: () => void | Promise<void>) => void,
  initial = '#1867c0',
) {
  const onValueChange = vi.fn<(hex: string) => void>()
  const view = await render(
    <Picker initial={initial} onValueChange={onValueChange} />,
  )
  onTestFinished(() => view.unmount())

  const slider = (name: string) =>
    view.getByRole('slider', { name }).element() as HTMLElement
  const value = (name: string) =>
    Math.round(Number(slider(name).getAttribute('aria-valuenow')))
  const hex = view.getByRole('textbox', { name: 'Hex' })
  const hexInput = () => hex.element() as HTMLInputElement

  return { view, onValueChange, slider, value, hex, hexInput }
}

test('parseHexColor accepts 3 and 6 digit hex and normalizes it', () => {
  expect(parseHexColor('#ABC')).toBe('#aabbcc')
  expect(parseHexColor(' 1867C0 ')).toBe('#1867c0')
  expect(parseHexColor('#1867c0')).toBe('#1867c0')

  for (const input of [
    '',
    '#',
    '#12',
    '#12345',
    '#1234567',
    '#11223344',
    '#ggg',
    'red',
    '##abc',
  ]) {
    expect(parseHexColor(input)).toBeNull()
  }
})

test('moving a slider emits the color of all three channels', async ({
  onTestFinished,
}) => {
  const { view, onValueChange, slider, value } = await mount(onTestFinished)
  const start = hctOf('#1867c0')

  await press(slider('Hue'), `{Home}${PAGE.repeat(3)}`)

  await expect
    .poll(() => onValueChange.mock.lastCall)
    .toEqual([hexOf(30, start.chroma, start.tone)])
  await expect.poll(() => value('Hue')).toBe(30)
  expect(
    view
      .getByRole('slider', { name: 'Hue' })
      .element()
      .closest('[data-slot=hct-color-picker-slider]')!
      .querySelector('[data-slot=hct-color-picker-slider-value]')!.textContent,
  ).toBe('30')
})

test('a chroma outside the sRGB gamut stays where it was put', async ({
  onTestFinished,
}) => {
  const { onValueChange, value, slider } = await mount(onTestFinished)

  await press(slider('Chroma'), '{End}')

  await expect.poll(() => onValueChange.mock.calls.length).toBe(1)
  // The emitted color is clipped to sRGB, far below the requested chroma…
  expect(hctOf(onValueChange.mock.lastCall![0]).chroma).toBeLessThan(100)
  // …yet coming back as the controlled value does not move the slider.
  await expect.poll(() => value('Chroma')).toBe(150)
})

test('typing a valid hex emits it normalized and moves the sliders', async ({
  onTestFinished,
}) => {
  const { onValueChange, value, hex } = await mount(onTestFinished)

  await hex.fill('#ABC')

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['#aabbcc'])
  const target = hctOf('#aabbcc')
  expect(value('Hue')).toBe(Math.round(target.hue))
  expect(value('Tone')).toBe(Math.round(target.tone))
})

test('an invalid hex is flagged, not emitted, and restored on blur', async ({
  onTestFinished,
}) => {
  const { onValueChange, hex, hexInput } = await mount(onTestFinished)

  await hex.fill('#12')

  await expect.poll(() => hexInput().getAttribute('aria-invalid')).toBe('true')
  expect(onValueChange).not.toHaveBeenCalled()

  hexInput().blur()

  await expect.poll(() => hexInput().value).toBe('#1867c0')
  expect(hexInput().hasAttribute('aria-invalid')).toBe(false)
})

test('outside changes do not overwrite the hex being edited', async ({
  onTestFinished,
}) => {
  const { value, hexInput } = await mount(onTestFinished)

  hexInput().focus()
  controls.setValue('#9e1e67')

  await expect.poll(() => value('Hue')).toBe(Math.round(hctOf('#9e1e67').hue))
  expect(hexInput().value).toBe('#1867c0')

  hexInput().blur()

  await expect.poll(() => hexInput().value).toBe('#9e1e67')
})

test('a gray keeps the hue that was chosen', async ({ onTestFinished }) => {
  const { slider, value, hex } = await mount(onTestFinished)

  await press(slider('Hue'), `{Home}${PAGE.repeat(20)}`)
  await expect.poll(() => value('Hue')).toBe(200)

  await hex.fill('#808080')

  await expect.poll(() => value('Tone')).toBe(Math.round(hctOf('#808080').tone))
  expect(value('Hue')).toBe(200)
})

test('the controlled value resyncs the sliders when it changes outside', async ({
  onTestFinished,
}) => {
  const { onValueChange, value } = await mount(onTestFinished)
  const target = hctOf('#9e1e67')

  controls.setValue('#9e1e67')

  await expect.poll(() => value('Hue')).toBe(Math.round(target.hue))
  expect(value('Chroma')).toBe(Math.round(target.chroma))
  expect(onValueChange).not.toHaveBeenCalled()
})

test('presets emit their color and report the checked one', async ({
  onTestFinished,
}) => {
  const { view, onValueChange } = await mount(onTestFinished)
  const accent = view.getByRole('radio', { name: 'Accent' })
  const plum = view.getByRole('radio', { name: '#9e1e67' })

  await expect
    .poll(() => accent.element().getAttribute('aria-checked'))
    .toBe('true')
  expect(plum.element().getAttribute('aria-checked')).toBe('false')
  expect(view.getByRole('radio', { name: 'Broken' }).query()).toBeNull()

  await plum.click()

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['#9e1e67'])
  await expect
    .poll(() => plum.element().getAttribute('aria-checked'))
    .toBe('true')
  expect(accent.element().getAttribute('aria-checked')).toBe('false')
})
