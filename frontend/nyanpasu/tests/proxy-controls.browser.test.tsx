import '@/assets/styles/tailwind.css'
import '@nyanpasu/theme/styles/theme.css'
import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { Card, CardContent, CardHeader } from '@nyanpasu/ui/card'
import { ExpressiveChoice } from '@nyanpasu/ui/expressive-choice'
import { ShapeToggle } from '@nyanpasu/ui/shape-toggle'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'

const options = ['Rule', 'Global', 'Direct'].map((value) => ({
  value,
  label: value,
  description: `Traffic routing: ${value}`,
  icon: (
    <svg viewBox="0 0 24 24" aria-hidden>
      <path fill="currentColor" d="M12 2 22 12 12 22 2 12Z" />
    </svg>
  ),
}))

function Controls({ layout }: { layout: 'focus' | 'flex' }) {
  const [value, setValue] = useState('Rule')
  const [active, setActive] = useState(false)
  return (
    <TooltipProvider>
      <div style={{ width: 304 }}>
        <Card
          className="flex flex-col"
          style={{ height: 144 }}
          data-testid="mode-card"
        >
          <CardHeader className="shrink-0 text-base" data-testid="mode-title">
            Proxy mode
          </CardHeader>
          <CardContent className="min-h-0 flex-1 overflow-hidden py-3">
            <ExpressiveChoice
              options={options}
              value={value}
              layout={layout}
              label="Proxy mode"
              onChange={setValue}
            />
          </CardContent>
        </Card>
        <Card
          className="mt-3 flex flex-col"
          style={{ height: 144, width: 224 }}
          data-testid="status-card"
        >
          <CardHeader className="shrink-0 text-base">Proxy status</CardHeader>
          <CardContent className="min-h-0 flex-1 flex-row items-stretch justify-center overflow-hidden py-3">
            <ShapeToggle
              active={active}
              icon={
                <svg viewBox="0 0 24 24" aria-hidden>
                  <path fill="currentColor" d="M12 2 22 12 12 22 2 12Z" />
                </svg>
              }
              label="System proxy"
              onClick={() => setActive(!active)}
            />
            <ShapeToggle
              active={false}
              icon={
                <svg viewBox="0 0 24 24" aria-hidden>
                  <path fill="currentColor" d="M12 2 22 12 12 22 2 12Z" />
                </svg>
              }
              label="TUN mode"
            />
          </CardContent>
        </Card>
      </div>
    </TooltipProvider>
  )
}

for (const layout of ['focus', 'flex'] as const) {
  test(`${layout} keeps titles and controls inside compact cards and animates selection`, async ({
    onTestFinished,
  }) => {
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    root.render(<Controls layout={layout} />)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })
    await expect
      .element(page.getByRole('button', { name: 'Direct' }))
      .toBeVisible()
    const title = container
      .querySelector('[data-testid="mode-title"]')!
      .getBoundingClientRect()
    const control = container
      .querySelector('[data-slot="expressive-choice"]')!
      .getBoundingClientRect()
    const card = container
      .querySelector('[data-testid="mode-card"]')!
      .getBoundingClientRect()
    expect(control.top).toBeGreaterThanOrEqual(title.bottom)
    expect(control.bottom).toBeLessThanOrEqual(card.bottom)
    const first = container.querySelector('[data-value="Rule"]')
    const before = first?.getBoundingClientRect().width
    const frames = new Set<string>()
    const recordAnimation =
      layout === 'focus'
        ? (async () => {
            for (let index = 0; index < 40; index++) {
              await new Promise<void>((resolve) =>
                requestAnimationFrame(() => resolve()),
              )
              const copy = container.querySelector(
                '[data-slot="expressive-choice-current-copy"]',
              )!
              frames.add(getComputedStyle(copy).transform)
            }
          })()
        : Promise.resolve()
    await userEvent.click(page.getByRole('button', { name: 'Direct' }))
    await expect
      .element(page.getByRole('button', { name: 'Direct' }))
      .toHaveAttribute('aria-pressed', 'true')
    if (layout === 'focus') {
      await recordAnimation
      expect(frames.size).toBeGreaterThan(1)
    }
    if (layout === 'flex')
      await expect
        .poll(() => first!.getBoundingClientRect().width)
        .toBeLessThan(before!)
    const toggle = page.getByRole('switch', { name: 'System proxy' })
    const shape = container.querySelector(
      '[data-slot="shape-toggle"] [data-slot="material-shape"]',
    )!
    const oldClip = getComputedStyle(shape).clipPath
    await userEvent.click(toggle)
    await expect.element(toggle).toHaveAttribute('aria-checked', 'true')
    await expect.poll(() => getComputedStyle(shape).clipPath).not.toBe(oldClip)
    for (const button of container.querySelectorAll(
      '[data-slot="shape-toggle"]',
    )) {
      const bounds = button.getBoundingClientRect()
      const status = container
        .querySelector('[data-testid="status-card"]')!
        .getBoundingClientRect()
      expect(bounds.bottom).toBeLessThanOrEqual(status.bottom)
      const surface = button
        .querySelector('[data-slot="shape-toggle-surface"]')!
        .getBoundingClientRect()
      expect(Math.abs(surface.width - surface.height)).toBeLessThan(1)
      const label = button.querySelector('[data-slot="shape-toggle-label"]')!
      expect(getComputedStyle(label).display).not.toBe('none')
      expect(label.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        status.bottom - 4,
      )
      expect(surface.bottom).toBeLessThanOrEqual(status.bottom - 4)
      expect(surface.top).toBeGreaterThanOrEqual(
        container
          .querySelector('[data-testid="status-card"] [class*="text-base"]')!
          .getBoundingClientRect().bottom,
      )
    }
    await expect
      .poll(
        () =>
          container.querySelector('[data-slot="expressive-choice-options"]')!
            .scrollWidth,
      )
      .toBeLessThanOrEqual(Math.ceil(control.width))
    let lastClip = ''
    let stableFrames = 0
    await expect
      .poll(() => {
        const clip = getComputedStyle(shape).clipPath
        stableFrames = clip === lastClip ? stableFrames + 1 : 0
        lastClip = clip
        return stableFrames
      })
      .toBeGreaterThanOrEqual(3)
    const wasDark = document.documentElement.classList.contains('dark')
    document.documentElement.classList.add('dark')
    onTestFinished(() => {
      document.documentElement.classList.toggle('dark', wasDark)
    })
    const selectedButton = container.querySelector<HTMLButtonElement>(
      '[data-slot="expressive-choice"] button[aria-pressed="true"]',
    )!
    const selectedShape = selectedButton.querySelector(
      '[data-slot="material-shape"]',
    )!
    if (layout === 'flex') {
      const badge = selectedShape.getBoundingClientRect()
      expect(Math.abs(badge.width - badge.height)).toBeLessThan(1)
      expect(badge.width).toBeLessThanOrEqual(48)
      expect(getComputedStyle(selectedButton).backgroundColor).not.toBe(
        'rgba(0, 0, 0, 0)',
      )
    }
    await expect
      .poll(
        () =>
          getComputedStyle(selectedButton).color ===
          getComputedStyle(selectedShape).backgroundColor,
      )
      .toBe(false)
    if (layout === 'focus')
      expect(getComputedStyle(selectedButton).backgroundColor).not.toBe(
        'rgba(0, 0, 0, 0)',
      )
    await page.screenshot({
      element: container,
      path: `../../../.vitest/proxy-controls-${layout}.png`,
    })
  })
}

test('keyboard focus survives a focus-card selection and pending controls cannot mutate', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(<Controls layout="focus" />)
  const direct = page.getByRole('button', { name: 'Direct' })
  await expect.element(direct).toBeVisible()
  direct.element().focus()
  await userEvent.keyboard('{Enter}')
  await expect
    .element(page.getByRole('button', { name: 'Direct' }))
    .toHaveAttribute('aria-pressed', 'true')
  await expect
    .poll(() => document.activeElement?.getAttribute('data-slot'))
    .toBe('expressive-choice-current')

  let changes = 0
  root.render(
    <TooltipProvider>
      <div>
        <ExpressiveChoice
          options={options}
          value="Rule"
          layout="flex"
          label="Proxy mode"
          disabled
          onChange={() => changes++}
        />
        <ShapeToggle
          active
          icon={<span>◈</span>}
          label="Pending proxy"
          loading
          onClick={() => changes++}
        />
      </div>
    </TooltipProvider>,
  )
  await expect
    .element(page.getByRole('switch', { name: 'Pending proxy' }))
    .toBeDisabled()
  await expect
    .element(page.getByRole('switch', { name: 'Pending proxy' }))
    .toHaveAttribute('aria-busy', 'true')
  await expect
    .element(page.getByRole('button', { name: 'Global' }))
    .toBeDisabled()
  for (const element of [
    page.getByRole('button', { name: 'Global' }).element(),
    page.getByRole('switch', { name: 'Pending proxy' }).element(),
  ]) {
    if (element instanceof HTMLButtonElement) element.click()
  }
  expect(changes).toBe(0)
})

test('compact status keeps small labels visible without tooltips', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  root.render(<Controls layout="flex" />)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  const toggle = page.getByRole('switch', { name: 'System proxy' })
  await userEvent.hover(toggle)
  expect(document.querySelector('[role="tooltip"]')).toBeNull()
  const label = container.querySelector('[data-slot="shape-toggle-label"]')!
  expect(getComputedStyle(label).display).not.toBe('none')
  expect(getComputedStyle(label).fontSize).toBe('11px')
  const card = container.querySelector<HTMLElement>(
    '[data-testid="status-card"]',
  )!
  card.style.height = '252px'
  await expect
    .poll(
      () =>
        getComputedStyle(
          card.querySelector('[data-slot="shape-toggle-label"]')!,
        ).display,
    )
    .not.toBe('none')
})

for (const active of [false, true]) {
  test(`loading ring preserves half-circle geometry when active=${active}`, async ({
    onTestFinished,
  }) => {
    const container = document.createElement('div')
    document.body.append(container)
    const root = createRoot(container)
    onTestFinished(() => {
      root.unmount()
      container.remove()
    })
    const render = (loading: boolean) =>
      root.render(
        <div style={{ width: 96, height: 80, display: 'flex' }}>
          <ShapeToggle
            active={active}
            loading={loading}
            label="Proxy"
            icon={
              <svg viewBox="0 0 24 24" data-testid="proxy-icon">
                <path d="M2 12h20" />
              </svg>
            }
          />
        </div>,
      )
    render(true)
    const toggle = page.getByRole('switch', { name: 'Proxy' })
    await expect.element(toggle).toHaveAttribute('aria-busy', 'true')
    await expect.element(toggle).toBeDisabled()
    const ring = container.querySelector('[data-slot="circular-progress"]')!
    const bounds = ring.getBoundingClientRect()
    expect(bounds.width).toBeGreaterThanOrEqual(16)
    expect(Math.abs(bounds.width - bounds.height)).toBeLessThan(1)
    for (const svg of ring.querySelectorAll('svg')) {
      expect(
        Math.abs(
          parseFloat(getComputedStyle(svg).width) -
            parseFloat(getComputedStyle(ring).width),
        ),
      ).toBeLessThan(1)
      expect(
        Math.abs(
          parseFloat(getComputedStyle(svg).height) -
            parseFloat(getComputedStyle(ring).height),
        ),
      ).toBeLessThan(1)
    }
    const spin = ring.querySelector(
      '[data-slot="circular-progress-indeterminate"]',
    )!
    const transforms = new Set<string>()
    for (let frame = 0; frame < 12; frame++) {
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      )
      transforms.add(getComputedStyle(spin).transform)
    }
    expect(transforms.size).toBeGreaterThan(1)
    render(false)
    await expect.element(toggle).toBeEnabled()
    await expect.element(page.getByTestId('proxy-icon')).toBeVisible()
    expect(
      container.querySelector('[data-slot="circular-progress"]'),
    ).toBeNull()
  })
}

test('elastic proxy status expands only the active button and shares space when states match', async ({
  onTestFinished,
}) => {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  for (const [system, tun] of [
    [false, false],
    [true, false],
    [true, true],
    [false, true],
  ]) {
    root.render(
      <div style={{ width: 224, height: 80, display: 'flex', gap: 8 }}>
        <ShapeToggle
          elastic
          active={system}
          label="System"
          icon={<span>◈</span>}
        />
        <ShapeToggle elastic active={tun} label="TUN" icon={<span>◈</span>} />
      </div>,
    )
    await expect
      .element(page.getByRole('switch', { name: 'System' }))
      .toHaveAttribute('aria-checked', String(system))
    expect(
      container.querySelectorAll('[data-slot="shape-toggle"]'),
    ).toHaveLength(2)
    expect(
      container.querySelectorAll('[data-slot="material-shape"]'),
    ).toHaveLength(2)
    for (const button of container.querySelectorAll(
      '[data-slot="shape-toggle"]',
    )) {
      expect(getComputedStyle(button).backgroundColor).not.toBe(
        'rgba(0, 0, 0, 0)',
      )
      expect(
        button
          .querySelector('[data-slot="material-shape"]')!
          .getBoundingClientRect().width,
      ).toBeLessThan(48)
    }
    await expect
      .poll(() => {
        const buttons = container.querySelectorAll('[data-slot="shape-toggle"]')
        const ratio =
          buttons[0].getBoundingClientRect().width /
          buttons[1].getBoundingClientRect().width
        return system === tun
          ? Math.abs(ratio - 1) < 0.02
          : system
            ? ratio > 1.5
            : ratio < 0.67
      })
      .toBe(true)
  }
})
