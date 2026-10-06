# HCT 主题颜色选择器与实时预览 任务

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用参照 material-web Theme Controls 的 HCT 选择器替换主题颜色下拉菜单，支持 hex 输入、实时预览与仅预览的颜色模式切换，点"应用"才保存颜色。

**Architecture:** `@nyanpasu/ui` 新增组合式 `HctColorPicker`（内部保存 H/C/T，输出 hex）；`ExperimentalThemeProvider` 新增不持久化的 `setThemePreview`，按"预览值 ?? 已保存值"绘制主题；设置页把下拉菜单换成 Popover 面板，草稿与模式预览只经由 `setThemePreview` 生效。

**Tech Stack:** React 19、Tailwind v4、`@material/material-color-utilities@0.4.0`、Radix Popover / ToggleGroup（经 `@nyanpasu/ui` 包装）、Vitest Browser Mode（Chromium）、Paraglide。

**Spec:** `docs/spec/2026-10-03-hct-theme-picker/design.md`（同目录，先读）

**Worktree:** `/Users/a632079/Programs/clash-nyanpasu-hct-picker`，分支 `feat/hct-color-picker`。所有命令都在 worktree 根目录执行。

## Global Constraints

- 遵守 `AGENTS.md` 与 `docs/development/typescript.md`：Prettier 风格（两空格、单引号、无分号、尾逗号）；按职责用空行分组；`data-slot` 用 kebab-case；不新增全局可变状态。
- `@nyanpasu/ui` 不得 import app、`@nyanpasu/rpc`、`@nyanpasu/query`、`@nyanpasu/platform`、Paraglide；所有文案通过 props 传入。
- Radix 只能在 `frontend/ui/` 内 import；app 只用 `@nyanpasu/ui/*` 组件。
- 输出 hex 一律是小写 6 位 `#rrggbb`；颜色比较一律忽略大小写（系统强调色是大写 `#RRGGBB`）。
- 颜色模式只预览：面板代码里不得出现 `setThemeMode` 调用。
- Paraglide 输出（`frontend/nyanpasu/src/paraglide`）与路由由 `pnpm web:build` 生成，不手改、不提交；改了 `messages/*.json` 后必须先跑 `pnpm web:build` 再跑测试和类型检查。
- `frontend/nyanpasu/src/generated/data-slots.gen.ts` 由 `deno task generate:data-slots` 生成并提交，不手改。
- 检查命令：`pnpm test:frontend <路径>`、`pnpm run-s lint:prettier lint:oxlint lint:ts lint:frontend-boundaries`。**不要**跑 `pnpm lint`（会触发 clippy，这个 worktree 没有 Rust 构建产物）。
- 提交只用显式路径 `git add <path>`，禁止 `git add .` / `-A`；提交信息为英文祈使句，≤72 字符，非平凡改动要有说明"为什么"的正文；不加任何 AI 署名行。
- 测试里不 sleep 等结果，用 `expect.poll`；唯一允许的短等待是"证明某件事**没有**发生"（与现有 `theme-provider.browser.test.tsx` 一致，50ms）。

## Review Focus

1. **应用后已保存值滞后：** `useSetting().upsert` resolve 时 refetch 还没回来，此时关闭面板会先闪回旧色 → Task 3 测试 "Apply saves the color once and closes after the saved value arrives"。
2. **面板打开时离开设置页：** 卸载必须撤销预览，否则整个应用停留在未保存的颜色 → Task 3 测试 "leaving the page with the panel open restores the saved theme"。
3. **清除预览后的那一帧：** `useDeferredValue` 会让主题在清除预览后仍按预览色算一帧，这一帧不得写入 localStorage 缓存 → Task 2 测试 "a color preview repaints without saving or caching it" 断言缓存最终是已保存色。
4. **已保存值大小写与草稿不同：** 已保存 `#1867C0`、草稿 `#1867c0` 时"应用"应禁用、系统强调色预设应选中 → Task 3 测试 "closing without applying restores the saved theme"。
5. **再次点击已选中的模式：** Radix ToggleGroup 会发出空字符串，模式不能变成"无" → Task 3 测试 "switching the color mode previews it and never saves it"。

---

## Task 1: `@nyanpasu/ui` 的 HCT 颜色选择器（提交 C1）

**Files:**

- Modify: `frontend/ui/package.json`（dependencies 加 `"@material/material-color-utilities": "0.4.0"`，按字母序放在 `@internationalized/date` 之后）
- Modify: `pnpm-lock.yaml`（由 `pnpm install` 更新）
- Create: `frontend/ui/src/hct-color-picker.tsx`
- Modify: `frontend/ui/src/index.ts`（按字母序在 `export * from './dropdown-menu'` 之后加 `export * from './hct-color-picker'`）
- Modify: `frontend/nyanpasu/src/generated/data-slots.gen.ts`（由生成器更新）
- Test: `frontend/ui/tests/hct-color-picker.browser.test.tsx`

**Interfaces:**

- Consumes: `Slider`（`frontend/ui/src/slider.tsx`，props：`value`、`min`、`max`、`step`、`onValueChange(value: number)`，其余 props 落到内部 `<input type="range">`）。
- Produces（Task 3 使用）：
  - `parseHexColor(input: string): string | null`
  - `HctColorPicker`：`Omit<ComponentProps<'div'>, 'defaultValue' | 'onChange'> & { value?: string; defaultValue?: string; onValueChange?: (hex: string) => void }`
  - `HctColorPickerPresets`：`ComponentProps<'div'>`（`role="radiogroup"`）
  - `HctColorPickerPreset`：`Omit<ComponentProps<'button'>, 'value' | 'children'> & { value: string; label?: string }`
  - `HctColorPickerHexField`：`Omit<ComponentProps<'div'>, 'children'> & { label: string }`
  - `HctColorPickerSliders`：`ComponentProps<'div'>`
  - `HctColorPickerSlider`：`Omit<ComponentProps<'div'>, 'children'> & { channel: HctChannel; label: string }`
  - `type HctChannel = 'hue' | 'chroma' | 'tone'`

- [ ] **Step 1: 加依赖**

编辑 `frontend/ui/package.json` 的 `dependencies`：

```json
    "@internationalized/date": "3.12.4",
    "@material/material-color-utilities": "0.4.0",
    "@nyanpasu/hooks": "workspace:^",
```

Run: `pnpm install`
Expected: 成功；`git diff --stat pnpm-lock.yaml` 只有 `frontend/ui` importer 的一处新增。

- [ ] **Step 2: 写失败的测试**

创建 `frontend/ui/tests/hct-color-picker.browser.test.tsx`：

```tsx
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
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

// React tracks an input's value; set it through the native setter so the
// dispatched event reaches onChange like a real drag.
const setRange = (input: HTMLInputElement, value: number) => {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    'value',
  )!.set!.call(input, String(value))
  input.dispatchEvent(new Event('input', { bubbles: true }))
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
    view.getByRole('slider', { name }).element() as HTMLInputElement
  const hex = view.getByRole('textbox', { name: 'Hex' })
  const hexInput = () => hex.element() as HTMLInputElement

  return { view, onValueChange, slider, hex, hexInput }
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
  const { onValueChange, slider } = await mount(onTestFinished)
  const start = hctOf('#1867c0')

  setRange(slider('Hue'), 30)

  await expect
    .poll(() => onValueChange.mock.lastCall)
    .toEqual([hexOf(30, start.chroma, start.tone)])
})

test('a chroma outside the sRGB gamut stays where it was put', async ({
  onTestFinished,
}) => {
  const { onValueChange, slider } = await mount(onTestFinished)

  setRange(slider('Chroma'), 150)

  await expect.poll(() => onValueChange.mock.calls.length).toBe(1)
  // The emitted color is clipped to sRGB, far below the requested chroma…
  expect(hctOf(onValueChange.mock.lastCall![0]).chroma).toBeLessThan(100)
  // …yet coming back as the controlled value does not move the slider.
  await expect.poll(() => slider('Chroma').value).toBe('150')
})

test('typing a valid hex emits it normalized and moves the sliders', async ({
  onTestFinished,
}) => {
  const { onValueChange, slider, hex } = await mount(onTestFinished)

  await hex.fill('#ABC')

  await expect.poll(() => onValueChange.mock.lastCall).toEqual(['#aabbcc'])
  const target = hctOf('#aabbcc')
  expect(slider('Hue').value).toBe(String(Math.round(target.hue)))
  expect(slider('Tone').value).toBe(String(Math.round(target.tone)))
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
  const { slider, hexInput } = await mount(onTestFinished)

  hexInput().focus()
  controls.setValue('#9e1e67')

  await expect
    .poll(() => slider('Hue').value)
    .toBe(String(Math.round(hctOf('#9e1e67').hue)))
  expect(hexInput().value).toBe('#1867c0')

  hexInput().blur()

  await expect.poll(() => hexInput().value).toBe('#9e1e67')
})

test('a gray keeps the hue that was chosen', async ({ onTestFinished }) => {
  const { slider, hex } = await mount(onTestFinished)

  setRange(slider('Hue'), 200)
  await expect.poll(() => slider('Hue').value).toBe('200')

  await hex.fill('#808080')

  await expect
    .poll(() => slider('Tone').value)
    .toBe(String(Math.round(hctOf('#808080').tone)))
  expect(slider('Hue').value).toBe('200')
})

test('the controlled value resyncs the sliders when it changes outside', async ({
  onTestFinished,
}) => {
  const { onValueChange, slider } = await mount(onTestFinished)
  const target = hctOf('#9e1e67')

  controls.setValue('#9e1e67')

  await expect
    .poll(() => slider('Hue').value)
    .toBe(String(Math.round(target.hue)))
  expect(slider('Chroma').value).toBe(String(Math.round(target.chroma)))
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
```

- [ ] **Step 3: 确认测试失败**

Run: `pnpm test:frontend frontend/ui/tests/hct-color-picker.browser.test.tsx`
Expected: FAIL，模块 `@nyanpasu/ui/hct-color-picker` 不存在。

- [ ] **Step 4: 实现组件**

创建 `frontend/ui/src/hct-color-picker.tsx`：

```tsx
import Check from '~icons/material-symbols/check-rounded'
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
import { Slider } from './slider'

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
        'bg-surface-variant text-on-surface-variant flex flex-col gap-6 rounded-3xl px-5 py-6',
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
      className={cn('flex flex-col gap-3', className)}
      {...props}
    >
      <div className="flex items-center justify-between text-sm">
        <span data-slot="hct-color-picker-slider-label">{label}</span>

        <span
          data-slot="hct-color-picker-slider-value"
          className="tabular-nums"
        >
          {Math.round(hct[channel])}
        </span>
      </div>

      <Slider
        aria-label={label}
        min={0}
        max={CHANNEL_MAX[channel]}
        value={hct[channel]}
        onValueChange={(channelValue) => setChannel(channel, channelValue)}
      />

      <div
        aria-hidden
        data-slot="hct-color-picker-gradient"
        className="border-outline h-6 rounded-full border"
        style={{ background: gradient }}
      />
    </div>
  )
}
```

编辑 `frontend/ui/src/index.ts`，在 `export * from './dropdown-menu'` 下一行加入：

```ts
export * from './hct-color-picker'
```

- [ ] **Step 5: 确认测试通过**

Run: `pnpm test:frontend frontend/ui/tests/hct-color-picker.browser.test.tsx`
Expected: 10 个测试全部 PASS。若某个断言失败，先判断是测试还是实现与 spec §3 不符，按 spec 修正；不要为了通过而放宽断言。

- [ ] **Step 6: 生成 data-slot 清单并跑检查**

Run: `deno task generate:data-slots`
Expected: `data-slots.gen.ts` 新增 `hct-color-picker`、`hct-color-picker-gradient`、`hct-color-picker-hex-field`、`hct-color-picker-hex-input`、`hct-color-picker-hex-label`、`hct-color-picker-preset`、`hct-color-picker-presets`、`hct-color-picker-slider`、`hct-color-picker-slider-label`、`hct-color-picker-slider-value`、`hct-color-picker-sliders`、`hct-color-picker-swatch`，没有其他变化。

Run: `pnpm exec prettier --write frontend/ui/src/hct-color-picker.tsx frontend/ui/tests/hct-color-picker.browser.test.tsx frontend/ui/src/index.ts frontend/ui/package.json`
Run: `pnpm run-s lint:prettier lint:oxlint lint:ts lint:frontend-boundaries`
Expected: 全部通过。

- [ ] **Step 7: 提交 C1**

```bash
git status --short
git add frontend/ui/package.json pnpm-lock.yaml frontend/ui/src/hct-color-picker.tsx frontend/ui/src/index.ts frontend/ui/tests/hct-color-picker.browser.test.tsx frontend/nyanpasu/src/generated/data-slots.gen.ts
git diff --cached --stat
git commit -F - <<'EOF'
feat(ui): add an HCT color picker

The theme color can only be picked by hue today, with fixed chroma and
tone. Material's HCT space describes a seed color by hue, chroma and tone,
which is what Material You derives the palette from, so picking in HCT
gives direct control over the result.

The picker keeps its three channels as state instead of deriving them
from the hex: HCT is wider than sRGB, so a chroma beyond the gamut is
clipped in the hex and would snap the slider back, and a gray has no hue
to recover. A hex field accepts typed colors.
EOF
```

---

## Task 2: provider 的不持久化预览（不单独提交，随 Task 3 进入 C2）

**Files:**

- Modify: `frontend/nyanpasu/src/components/providers/theme-provider.tsx`
- Test: `frontend/nyanpasu/tests/theme-provider.browser.test.tsx`

**Interfaces:**

- Produces（Task 3 使用）：
  - `export type ThemePreview = { color?: string; mode?: ThemeMode }`
  - context 新增 `setThemePreview: (preview: ThemePreview | null) => void`（稳定引用）
  - 语义：`themeColor` / `themeMode` 仍是已保存值；`themePalette` / `themeCssVars` / `currentThemeMode` 是生效值。

- [ ] **Step 1: 写失败的测试**

编辑 `frontend/nyanpasu/tests/theme-provider.browser.test.tsx`：

1. 把 `@nyanpasu/query` 的 mock 改成共享的 `upsert` spy（保留"每次调用返回新对象"）：

```tsx
const upsert = vi.hoisted(() => vi.fn(async () => {}))
// Like the real hook, every call returns a new object.
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: keyof typeof settings) => ({
    value: settings[key],
    upsert,
  }),
}))
```

2. 在 import 区加入 `import { createTheme, ThemeMode } from '@nyanpasu/theme'`。

3. 让 `Reader` 同时保存 context：

```tsx
let context: ReturnType<typeof useExperimentalThemeContext> | undefined
function Reader() {
  context = useExperimentalThemeContext()
  palette = context.themePalette
  return null
}
```

4. 在文件末尾追加：

```tsx
test('a color preview repaints without saving or caching it', async ({
  onTestFinished,
}) => {
  localStorage.clear()
  upsert.mockClear()
  settings.theme_mode = 'light'
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  const saved = createTheme('#ff0000').cssVars
  const preview = createTheme('#0000ff').cssVars
  await expect.poll(() => customStyle()).toBe(saved)
  await expect
    .poll(() => localStorage.getItem(CSS_VARS_KEY))
    .toBe(JSON.stringify(saved))
  const setItem = vi.spyOn(Storage.prototype, 'setItem')
  onTestFinished(() => setItem.mockRestore())

  context!.setThemePreview({ color: '#0000ff' })

  await expect.poll(() => customStyle()).toBe(preview)
  expect(context!.themeColor).toBe('#ff0000')
  expect(setItem).not.toHaveBeenCalled()

  context!.setThemePreview(null)

  await expect.poll(() => customStyle()).toBe(saved)
  // The frame that still paints the deferred preview color is not cached.
  await expect
    .poll(() => localStorage.getItem(CSS_VARS_KEY))
    .toBe(JSON.stringify(saved))
  expect(
    setItem.mock.calls.some(([, value]) => value === JSON.stringify(preview)),
  ).toBe(false)
  expect(upsert).not.toHaveBeenCalled()
})

test('a mode preview switches the color scheme without saving it', async ({
  onTestFinished,
}) => {
  upsert.mockClear()
  settings.theme_mode = 'light'
  settings.theme_color = '#ff0000'
  const view = await render(<Harness />)
  onTestFinished(() => view.unmount())
  const root = document.documentElement
  await expect.poll(() => root.classList.contains('light')).toBe(true)

  context!.setThemePreview({ mode: ThemeMode.DARK })

  await expect.poll(() => root.classList.contains('dark')).toBe(true)
  expect(root.classList.contains('light')).toBe(false)
  expect(context!.themeMode).toBe(ThemeMode.LIGHT)
  expect(context!.currentThemeMode).toBe(ThemeMode.DARK)

  context!.setThemePreview(null)

  await expect.poll(() => root.classList.contains('light')).toBe(true)
  expect(root.classList.contains('dark')).toBe(false)
  expect(upsert).not.toHaveBeenCalled()
})
```

- [ ] **Step 2: 确认测试失败**

Run: `pnpm test:frontend frontend/nyanpasu/tests/theme-provider.browser.test.tsx`
Expected: 两个新测试 FAIL（`setThemePreview` 不是函数）；原有两个测试 PASS。

- [ ] **Step 3: 实现**

编辑 `frontend/nyanpasu/src/components/providers/theme-provider.tsx`：

1. React import 加 `useDeferredValue`。

2. 在 `ThemeContext` 之前加入类型，并给 context 类型加字段：

```tsx
export type ThemePreview = {
  color?: string
  mode?: ThemeMode
}

const ThemeContext = createContext<{
  themePalette: Theme
  themeCssVars: string
  themeColor: string
  setThemeColor: (color: string) => Promise<void>
  themeMode: ThemeMode
  currentThemeMode: ResolvedThemeMode
  setThemeMode: (mode: ThemeMode) => Promise<void>
  setThemePreview: (preview: ThemePreview | null) => void
} | null>(null)
```

3. 在 `ExperimentalThemeProvider` 里，`themeColor` 之后加预览状态与生效值，并把 `theme` 改为按生效颜色计算：

```tsx
const themeColor = useSetting('theme_color')
const [resolvedThemeMode, setResolvedThemeMode] =
  useState<ResolvedThemeMode>(getSystemThemeMode())

// An unsaved color / mode shown on top of the saved settings.
const [preview, setPreview] = useState<ThemePreview | null>(null)

// Dragging a color changes it faster than themes rebuild; let React skip
// the intermediate ones.
const effectiveColor = useDeferredValue(preview?.color ?? themeColor.value)
const effectiveMode = preview?.mode ?? themeMode.value

// The theme of the last session paints the window until the settings have
// loaded; `theme_color` is always present once they have.
const [cachedTheme] = useState(readCachedTheme)

const theme = useMemo(
  () =>
    effectiveColor === undefined ? cachedTheme : createTheme(effectiveColor),
  [effectiveColor, cachedTheme],
)

// Only the saved color is cached for the next launch, never a preview
// (including the deferred frame right after a preview is cleared).
const isSavedTheme = effectiveColor === themeColor.value
```

4. 缓存 effect 增加条件：

```tsx
useEffect(() => {
  if (theme === cachedTheme || !isSavedTheme) {
    return
  }

  try {
    localStorage.setItem(THEME_PALETTE_KEY, JSON.stringify(theme.palette))
    localStorage.setItem(THEME_CSS_VARS_KEY, JSON.stringify(theme.cssVars))
  } catch {
    // ignore quota / security errors
  }
}, [theme, cachedTheme, isSavedTheme])
```

5. 在 `setThemeColor` 之后加入：

```tsx
const setThemePreview = useCallback((next: ThemePreview | null) => {
  setPreview((current) =>
    current?.color === next?.color && current?.mode === next?.mode
      ? current
      : next,
  )
}, [])
```

6. "initialize theme mode on mount" effect 与 "listen to theme changed event" effect 中，把每一处 `themeMode.value` 换成 `effectiveMode`（包括依赖数组）。`setThemeMode` 不变。

7. `currentThemeMode` 的 `useMemo` 中把 `themeMode.value` 换成 `effectiveMode`（包括依赖数组）。

8. context value 加入 `setThemePreview`（对象与依赖数组都加）。`themeColor: color` 与 `themeMode: themeMode.value as ThemeMode` 保持不变。

- [ ] **Step 4: 确认测试通过**

Run: `pnpm test:frontend frontend/nyanpasu/tests/theme-provider.browser.test.tsx`
Expected: 4 个测试全部 PASS（原有的"重复渲染不重建主题、不写缓存"必须仍然通过）。

- [ ] **Step 5: 检查（不提交）**

Run: `pnpm exec prettier --write frontend/nyanpasu/src/components/providers/theme-provider.tsx frontend/nyanpasu/tests/theme-provider.browser.test.tsx`
Run: `pnpm run-s lint:oxlint lint:ts`
Expected: 通过。**不要提交**：provider 的预览接口和 Task 3 的面板一起进入 C2。

---

## Task 3: 主题颜色面板（提交 C2，包含 Task 2）

**Files:**

- Modify: `frontend/nyanpasu/src/pages/(main)/main/settings/user-interface/_modules/theme-color-config.tsx`（整体重写）
- Modify: `frontend/nyanpasu/messages/en.json`、`zh-cn.json`、`zh-tw.json`、`ko.json`、`ru.json`
- Modify: `frontend/nyanpasu/package.json`（删除 `"@uiw/react-color": "2.10.3"`）
- Modify: `pnpm-lock.yaml`（由 `pnpm install` 更新）
- Modify: `frontend/nyanpasu/src/generated/data-slots.gen.ts`（由生成器更新）
- Test: `frontend/nyanpasu/tests/theme-color-config.browser.test.tsx`

**Interfaces:**

- Consumes: Task 1 的 `HctColorPicker*` 部件（见 Task 1 Produces）；Task 2 的 `setThemePreview`、`ThemePreview`；现有 `Popover` / `PopoverTrigger` / `PopoverContent`（`@nyanpasu/ui/popover`，`PopoverContent` 的 `className` 落到面板表面，默认 `w-72`）；`SegmentedButton` / `SegmentedButtonItem`（`@nyanpasu/ui/segmented-button`，单选；再次点击已选项会以 `''` 调用 `onValueChange`）；`Button`（`loading`、`disabled`、`variant="flat"`）；`message`（`@/utils/notification`）；`formatError`（`@/utils`）。

- [ ] **Step 1: 文案**

在 5 个语言文件里，紧接 `settings_user_interface_theme_color_system_accent` 一行之后插入下列 5 个 key，并删除 `settings_user_interface_theme_color_custom` 一行：

`en.json`：

```json
  "settings_user_interface_theme_color_system_accent": "System Accent Color",
  "settings_user_interface_theme_color_presets": "Presets",
  "settings_user_interface_theme_color_hex_source": "Hex Source Color",
  "settings_user_interface_theme_color_hue": "Hue",
  "settings_user_interface_theme_color_chroma": "Chroma",
  "settings_user_interface_theme_color_tone": "Tone",
```

`zh-cn.json`：

```json
  "settings_user_interface_theme_color_system_accent": "系统强调色",
  "settings_user_interface_theme_color_presets": "预设",
  "settings_user_interface_theme_color_hex_source": "Hex 源颜色",
  "settings_user_interface_theme_color_hue": "色相",
  "settings_user_interface_theme_color_chroma": "色度",
  "settings_user_interface_theme_color_tone": "色调",
```

`zh-tw.json`：

```json
  "settings_user_interface_theme_color_system_accent": "系統強調色",
  "settings_user_interface_theme_color_presets": "預設",
  "settings_user_interface_theme_color_hex_source": "Hex 來源顏色",
  "settings_user_interface_theme_color_hue": "色相",
  "settings_user_interface_theme_color_chroma": "彩度",
  "settings_user_interface_theme_color_tone": "色調",
```

`ko.json`：

```json
  "settings_user_interface_theme_color_system_accent": "시스템 강조 색상",
  "settings_user_interface_theme_color_presets": "프리셋",
  "settings_user_interface_theme_color_hex_source": "Hex 소스 색상",
  "settings_user_interface_theme_color_hue": "색조",
  "settings_user_interface_theme_color_chroma": "채도",
  "settings_user_interface_theme_color_tone": "명도",
```

`ru.json`：

```json
  "settings_user_interface_theme_color_system_accent": "Системный акцентный цвет",
  "settings_user_interface_theme_color_presets": "Пресеты",
  "settings_user_interface_theme_color_hex_source": "Исходный цвет (Hex)",
  "settings_user_interface_theme_color_hue": "Оттенок",
  "settings_user_interface_theme_color_chroma": "Насыщенность",
  "settings_user_interface_theme_color_tone": "Тон",
```

确认 `settings_user_interface_theme_color_custom` 在 `frontend/` 下已无其他引用：`grep -rn "theme_color_custom" frontend --include=*.ts --include=*.tsx --include=*.json | grep -v node_modules | grep -v src/paraglide`（重写面板后应为空）。

Run: `pnpm web:build`
Expected: 成功，`frontend/nyanpasu/src/paraglide/messages` 中出现新 key（该目录不提交）。

- [ ] **Step 2: 写失败的测试**

创建 `frontend/nyanpasu/tests/theme-color-config.browser.test.tsx`：

```tsx
import { useState } from 'react'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { userEvent } from 'vitest/browser'
import { ExperimentalThemeProvider } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { createTheme } from '@nyanpasu/theme'
import ThemeColorConfig from '../src/pages/(main)/main/settings/user-interface/_modules/theme-color-config'

const backend = vi.hoisted(() => ({
  settings: {} as Record<string, string>,
  save: vi.fn<(key: string, value: string) => Promise<void>>(),
  // When false, a saved value reaches the hook only on commitSaved(), like
  // the refetch that lands after the mutation has resolved.
  autoCommit: true,
  commitSaved: () => {},
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: (key: string) => {
    const [value, setValue] = useState(backend.settings[key])
    return {
      value,
      upsert: async (next: string) => {
        await backend.save(key, next)
        if (backend.autoCommit) {
          setValue(next)
        } else {
          backend.commitSaved = () => setValue(next)
        }
      },
    }
  },
  useSystemAccentColor: () => ({ systemAccentColor: '#1867C0' }),
}))
const notify = vi.hoisted(() => vi.fn())
vi.mock('@/utils/notification', () => ({ message: notify }))

const SAVED = '#1867C0'

const customStyle = () => document.getElementById('custom-theme')?.innerHTML

// React tracks an input's value; set it through the native setter so the
// dispatched event reaches onChange like a real drag.
const setRange = (input: HTMLInputElement, value: number) => {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    'value',
  )!.set!.call(input, String(value))
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

const page = { leave: () => {} }

function Page() {
  const [shown, setShown] = useState(true)
  page.leave = () => setShown(false)

  return (
    <ExperimentalThemeProvider>
      {shown && <ThemeColorConfig />}
    </ExperimentalThemeProvider>
  )
}

async function openPanel(
  onTestFinished: (fn: () => void | Promise<void>) => void,
) {
  backend.settings = { theme_color: SAVED, theme_mode: 'light' }
  backend.autoCommit = true
  backend.save.mockReset()
  backend.save.mockResolvedValue(undefined)
  notify.mockReset()

  const view = await render(<Page />)
  onTestFinished(() => view.unmount())

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)

  await view
    .getByRole('button', {
      name: new RegExp(m.settings_user_interface_theme_color_label()),
    })
    .click()

  const hex = view.getByRole('textbox', {
    name: m.settings_user_interface_theme_color_hex_source(),
  })
  await expect.element(hex).toBeVisible()

  return {
    view,
    hex,
    hexInput: () => hex.element() as HTMLInputElement,
    apply: view.getByRole('button', { name: m.common_apply() }),
  }
}

test('dragging, typing and presets preview the theme without saving it', async ({
  onTestFinished,
}) => {
  const { view, hex, hexInput } = await openPanel(onTestFinished)

  setRange(
    view
      .getByRole('slider', {
        name: m.settings_user_interface_theme_color_hue(),
      })
      .element() as HTMLInputElement,
    30,
  )
  await expect.poll(() => hexInput().value).not.toBe('#1867c0')
  await expect.poll(customStyle).toBe(createTheme(hexInput().value).cssVars)

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)

  await view.getByRole('radio', { name: '#9e1e67' }).click()
  await expect.poll(customStyle).toBe(createTheme('#9e1e67').cssVars)

  expect(backend.save).not.toHaveBeenCalled()
})

test('switching the color mode previews it and never saves it', async ({
  onTestFinished,
}) => {
  const { view } = await openPanel(onTestFinished)
  const root = document.documentElement
  const dark = view.getByRole('radio', {
    name: m.settings_user_interface_theme_mode_dark(),
  })

  await dark.click()
  await expect.poll(() => root.classList.contains('dark')).toBe(true)

  // Clicking the selected mode again keeps it selected.
  await dark.click()
  await expect
    .poll(() => dark.element().getAttribute('aria-checked'))
    .toBe('true')
  expect(root.classList.contains('dark')).toBe(true)

  await userEvent.keyboard('{Escape}')

  await expect.poll(() => root.classList.contains('light')).toBe(true)
  expect(root.classList.contains('dark')).toBe(false)
  expect(backend.save).not.toHaveBeenCalled()
})

test('Apply saves the color once and closes after the saved value arrives', async ({
  onTestFinished,
}) => {
  const { hex, apply } = await openPanel(onTestFinished)
  backend.autoCommit = false

  await hex.fill('#00ff00')
  await apply.click()

  await expect
    .poll(() => backend.save.mock.calls)
    .toEqual([['theme_color', '#00ff00']])
  // The mutation resolved but the refetch has not landed: closing now
  // would flash the old color.
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(hex.query()).not.toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)

  backend.commitSaved()

  await expect.poll(() => hex.query()).toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)
})

test('closing without applying restores the saved theme', async ({
  onTestFinished,
}) => {
  const { view, hex, apply } = await openPanel(onTestFinished)

  // The saved color differs from the picker's lowercase hex only in case.
  await expect.element(apply).toBeDisabled()
  expect(
    view
      .getByRole('radio', {
        name: m.settings_user_interface_theme_color_system_accent(),
      })
      .element()
      .getAttribute('aria-checked'),
  ).toBe('true')

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)
  await expect.element(apply).toBeEnabled()

  await userEvent.keyboard('{Escape}')

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)
  expect(backend.save).not.toHaveBeenCalled()
})

test('leaving the page with the panel open restores the saved theme', async ({
  onTestFinished,
}) => {
  const { hex } = await openPanel(onTestFinished)

  await hex.fill('#00ff00')
  await expect.poll(customStyle).toBe(createTheme('#00ff00').cssVars)

  page.leave()

  await expect.poll(customStyle).toBe(createTheme(SAVED).cssVars)
  expect(backend.save).not.toHaveBeenCalled()
})

test('a failed save keeps the panel open and reports the error', async ({
  onTestFinished,
}) => {
  const { hex, apply } = await openPanel(onTestFinished)
  backend.save.mockRejectedValue(new Error('boom'))

  await hex.fill('#00ff00')
  await apply.click()

  await expect.poll(() => notify.mock.calls.length).toBe(1)
  expect(hex.query()).not.toBeNull()
  expect(customStyle()).toBe(createTheme('#00ff00').cssVars)
  await expect.element(apply).toBeEnabled()
})
```

- [ ] **Step 3: 确认测试失败**

Run: `pnpm test:frontend frontend/nyanpasu/tests/theme-color-config.browser.test.tsx`
Expected: FAIL（面板仍是下拉菜单，找不到 hex 输入框）。

- [ ] **Step 4: 重写面板**

把 `theme-color-config.tsx` 整体替换为：

```tsx
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded'
import BrightnessMediumRounded from '~icons/material-symbols/brightness-medium-rounded'
import DarkModeRounded from '~icons/material-symbols/dark-mode-rounded'
import LightModeRounded from '~icons/material-symbols/light-mode-rounded'
import { useEffect, useState } from 'react'
import { Button } from '@nyanpasu/ui/button'
import {
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerPreset,
  HctColorPickerPresets,
  HctColorPickerSlider,
  HctColorPickerSliders,
} from '@nyanpasu/ui/hct-color-picker'
import { Popover, PopoverContent, PopoverTrigger } from '@nyanpasu/ui/popover'
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@nyanpasu/ui/segmented-button'
import { useExperimentalThemeContext } from '@/components/providers/theme-provider'
import { m } from '@/paraglide/messages'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useSystemAccentColor } from '@nyanpasu/query'
import { ThemeMode } from '@nyanpasu/theme'
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card'

const PERSETS = ['#9e1e67', '#3d009e', '#00089e', '#066b9e', '#9e5a00']

// The system accent color arrives as uppercase `#RRGGBB`; the picker emits
// lowercase.
const isSameColor = (a: string, b: string) =>
  a.toLowerCase() === b.toLowerCase()

export default function ThemeColorConfig() {
  const { themeColor, themeMode, setThemeColor, setThemePreview } =
    useExperimentalThemeContext()

  const { systemAccentColor } = useSystemAccentColor()

  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState(themeColor)
  const [previewMode, setPreviewMode] = useState<ThemeMode>()
  const [applying, setApplying] = useState(false)
  const [closeWhenSaved, setCloseWhenSaved] = useState(false)

  useEffect(() => {
    setThemePreview(open ? { color: draft, mode: previewMode } : null)
  }, [open, draft, previewMode, setThemePreview])

  // Leaving the page with the panel open must not keep the preview.
  useEffect(() => () => setThemePreview(null), [setThemePreview])

  // `upsert` resolves before the saved value is refetched; closing (and so
  // dropping the preview) any earlier would flash the old color.
  useEffect(() => {
    if (closeWhenSaved && isSameColor(themeColor, draft)) {
      setCloseWhenSaved(false)
      setOpen(false)
    }
  }, [closeWhenSaved, themeColor, draft])

  const modes = [
    {
      value: ThemeMode.DARK,
      label: m.settings_user_interface_theme_mode_dark(),
      Icon: DarkModeRounded,
    },
    {
      value: ThemeMode.SYSTEM,
      label: m.settings_user_interface_theme_mode_system(),
      Icon: BrightnessMediumRounded,
    },
    {
      value: ThemeMode.LIGHT,
      label: m.settings_user_interface_theme_mode_light(),
      Icon: LightModeRounded,
    },
  ]

  const handleOpenChange = (nextOpen: boolean) => {
    // Closing mid-save would drop the preview before the color is saved.
    if (applying) {
      return
    }

    if (nextOpen) {
      setDraft(themeColor)
      setPreviewMode(undefined)
    }

    setCloseWhenSaved(false)
    setOpen(nextOpen)
  }

  const handleDraftChange = (color: string) => {
    setDraft(color)
    setCloseWhenSaved(false)
  }

  const handleModeChange = (value: string) => {
    // Clicking the selected mode again deselects it; keep one selected.
    if (value) {
      setPreviewMode(value as ThemeMode)
    }
  }

  const handleApply = async () => {
    setApplying(true)

    try {
      await setThemeColor(draft)
      setCloseWhenSaved(true)
    } catch (error) {
      message(`Update theme color failed!\n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
        error,
      })
    } finally {
      setApplying(false)
    }
  }

  return (
    <SettingsCard data-slot="theme-color-config-card">
      <Popover open={open} onOpenChange={handleOpenChange}>
        <PopoverTrigger asChild>
          <SettingsCardContent data-slot="theme-color-config-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_user_interface_theme_color_label()}
                  </ItemLabelText>

                  <ItemLabelDescription className="space-x-1.5">
                    <span
                      className="bg-primary inline-block size-3 rounded-full"
                      data-slot="theme-color-config-colorful-preview"
                      style={{
                        backgroundColor: themeColor,
                      }}
                    />

                    <span>{themeColor}</span>
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </PopoverTrigger>

        <PopoverContent align="end" className="w-80">
          <HctColorPicker
            className="min-h-0 overflow-y-auto p-4"
            value={draft}
            onValueChange={handleDraftChange}
          >
            <HctColorPickerPresets
              aria-label={m.settings_user_interface_theme_color_presets()}
            >
              {PERSETS.map((value) => (
                <HctColorPickerPreset key={value} value={value} />
              ))}

              {systemAccentColor && (
                <HctColorPickerPreset
                  value={systemAccentColor}
                  label={m.settings_user_interface_theme_color_system_accent()}
                />
              )}
            </HctColorPickerPresets>

            <HctColorPickerHexField
              label={m.settings_user_interface_theme_color_hex_source()}
            />

            <HctColorPickerSliders>
              <HctColorPickerSlider
                channel="hue"
                label={m.settings_user_interface_theme_color_hue()}
              />

              <HctColorPickerSlider
                channel="chroma"
                label={m.settings_user_interface_theme_color_chroma()}
              />

              <HctColorPickerSlider
                channel="tone"
                label={m.settings_user_interface_theme_color_tone()}
              />
            </HctColorPickerSliders>

            <SegmentedButton
              data-slot="theme-color-config-mode-preview"
              aria-label={m.settings_user_interface_theme_mode_label()}
              value={previewMode ?? themeMode}
              onValueChange={handleModeChange}
            >
              {modes.map(({ value, label, Icon }) => (
                <SegmentedButtonItem
                  key={value}
                  value={value}
                  aria-label={label}
                  title={label}
                >
                  <Icon className="size-5" />
                </SegmentedButtonItem>
              ))}
            </SegmentedButton>

            <Button
              data-slot="theme-color-config-apply"
              className="self-end"
              variant="flat"
              disabled={applying || isSameColor(draft, themeColor)}
              loading={applying}
              onClick={handleApply}
            >
              {m.common_apply()}
            </Button>
          </HctColorPicker>
        </PopoverContent>
      </Popover>
    </SettingsCard>
  )
}
```

- [ ] **Step 5: 删除 `@uiw/react-color` 并确认测试通过**

确认无其他引用：`grep -rn "@uiw/react-color" frontend --include=*.ts --include=*.tsx | grep -v node_modules`（应为空）。从 `frontend/nyanpasu/package.json` 的 `dependencies` 删除 `"@uiw/react-color": "2.10.3",` 一行。

Run: `pnpm install`
Expected: 成功；`pnpm-lock.yaml` 移除 `@uiw/react-color` 及其不再需要的传递依赖。

Run: `pnpm test:frontend frontend/nyanpasu/tests/theme-color-config.browser.test.tsx frontend/nyanpasu/tests/theme-provider.browser.test.tsx frontend/ui/tests/hct-color-picker.browser.test.tsx`
Expected: 全部 PASS。

- [ ] **Step 6: 生成 data-slot 清单、全量测试与检查**

Run: `deno task generate:data-slots`
Expected: 新增 `theme-color-config-apply`、`theme-color-config-mode-preview`、`theme-color-config-trigger`；删除 `theme-color-config-colorful-custom-preview`、`theme-color-config-colorful-select-preview`、`theme-color-config-colorful-system-accent-preview`（若其他文件仍使用 `theme-mode-selector-trigger`，它会保留——`theme-mode-selector.tsx` 仍在用，属正常）。

Run: `pnpm exec prettier --write "frontend/nyanpasu/src/pages/(main)/main/settings/user-interface/_modules/theme-color-config.tsx" frontend/nyanpasu/tests/theme-color-config.browser.test.tsx frontend/nyanpasu/messages frontend/nyanpasu/package.json`
Run: `pnpm test:frontend`
Expected: 全部 PASS（如有与本改动无关的既有失败，记录测试名与输出，不要修改无关代码）。
Run: `pnpm run-s lint:prettier lint:oxlint lint:ts lint:frontend-boundaries`
Expected: 全部通过。

- [ ] **Step 7: 提交 C2**

```bash
git status --short
git add frontend/nyanpasu/src/components/providers/theme-provider.tsx frontend/nyanpasu/tests/theme-provider.browser.test.tsx "frontend/nyanpasu/src/pages/(main)/main/settings/user-interface/_modules/theme-color-config.tsx" frontend/nyanpasu/tests/theme-color-config.browser.test.tsx frontend/nyanpasu/messages/en.json frontend/nyanpasu/messages/zh-cn.json frontend/nyanpasu/messages/zh-tw.json frontend/nyanpasu/messages/ko.json frontend/nyanpasu/messages/ru.json frontend/nyanpasu/package.json pnpm-lock.yaml frontend/nyanpasu/src/generated/data-slots.gen.ts
git diff --cached --stat
git commit -F - <<'EOF'
feat(settings): preview theme color and mode live before applying

Choosing a theme color saved it immediately, and the custom option only
offered a hue strip that started from red regardless of the current
color, so the only way to judge a color was to save it.

The theme color now opens an HCT panel with presets, an editable hex
field and hue/chroma/tone sliders. Every change repaints the app as an
unsaved preview held by the theme provider; only Apply saves the color.
The color mode can be previewed in the same panel but is never saved
there. Closing the panel or leaving the page drops the preview, and a
preview is never written to the theme cache that paints the next launch.
EOF
```
