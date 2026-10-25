import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import type { ComponentProps } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import YamlViewer from '../src/pages/(main)/main/profiles/inspect/_modules/yaml-viewer'

const document_ = (lines: number) =>
  Array.from({ length: lines }, (_, i) => `key-${i}: value`).join('\n')

async function renderViewer(
  props: Partial<ComponentProps<typeof YamlViewer>> & { code: string },
  onTestFinished: (callback: () => void) => void,
) {
  const container = document.createElement('div')
  // A bounded height makes the region scroll, as it does in the page.
  container.style.cssText =
    'display:flex;flex-direction:column;height:300px;width:600px'
  document.body.append(container)
  const root = createRoot(container)
  const client = new QueryClient()
  const view = (extra: Partial<ComponentProps<typeof YamlViewer>>) => (
    <QueryClientProvider client={client}>
      <YamlViewer label="YAML" cacheKey={['test']} {...props} {...extra} />
    </QueryClientProvider>
  )
  root.render(view({}))
  onTestFinished(() => {
    root.unmount()
    container.remove()
    client.clear()
  })
  const region = await vi.waitUntil(() =>
    container.querySelector<HTMLElement>('[data-slot="yaml-viewer"]'),
  )
  return {
    container,
    region,
    update: (extra: Partial<ComponentProps<typeof YamlViewer>>) =>
      root.render(view(extra)),
  }
}

const bar = (region: HTMLElement, selector: string) =>
  region.querySelector<HTMLElement>(selector)

const middle = (rect: DOMRect) => (rect.top + rect.bottom) / 2

/**
 * The vertical middle of a line of text, measured from the rendered text once
 * the line is mounted.
 */
async function lineMiddle(region: HTMLElement, line: number) {
  const text = await vi.waitUntil(() =>
    region.querySelector(`pre .line[data-line="${line + 1}"]`),
  )
  return middle(text.getBoundingClientRect())
}

test('a large document is shown as plain text', async ({ onTestFinished }) => {
  const { container, region } = await renderViewer(
    { code: document_(100_001) },
    onTestFinished,
  )
  await vi.waitUntil(() => region.querySelector('pre code'))
  // Highlighting is not even attempted past the cap.
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(container.querySelector('[data-highlighted]')).toBeNull()
  expect(region.querySelector('pre .line')?.textContent).toBe('key-0: value')
})

test('a document of tens of thousands of lines is highlighted', async ({
  onTestFinished,
}) => {
  const { container } = await renderViewer(
    { code: document_(30_000) },
    onTestFinished,
  )
  await vi.waitUntil(() => container.querySelector('[data-highlighted]'), {
    timeout: 10_000,
  })
})

test('a small document is highlighted', async ({ onTestFinished }) => {
  const { container } = await renderViewer(
    { code: document_(50) },
    onTestFinished,
  )
  // The first document highlighted in a run also loads the highlighter.
  await vi.waitUntil(() => container.querySelector('[data-highlighted]'), {
    timeout: 5000,
  })
})

test('a document is highlighted again in the colors of a new color scheme', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer({ code: document_(50) }, onTestFinished)
  onTestFinished(() => document.documentElement.classList.remove('dark'))
  const keyColor = () =>
    region.querySelector<HTMLElement>('[data-highlighted] .line span')?.style
      .color
  const light = await vi.waitUntil(keyColor, { timeout: 5000 })

  document.documentElement.classList.add('dark')

  await vi.waitUntil(() => keyColor() && keyColor() !== light)
})

test('find marks the matching lines at the lines they are on', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer(
    { code: document_(60), find: { query: 'key-4', active: 3 } },
    onTestFinished,
  )
  await vi.waitUntil(() => region.querySelector('[data-highlighted]'))

  // key-4 and key-40 .. key-49.
  expect(region.querySelectorAll('[data-find]')).toHaveLength(11)
  const active = bar(region, '[data-find-active]')!
  // The fourth match is key-42, on the 43rd line.
  expect(active.dataset.line).toBe('42')
  expect(
    Math.abs(
      middle(active.getBoundingClientRect()) - (await lineMiddle(region, 42)),
    ),
  ).toBeLessThan(1)
})

test('the bars line up with the plain text of a large document', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer(
    { code: document_(4200), find: { query: 'key-4100:', active: 0 } },
    onTestFinished,
  )
  const active = (await vi.waitUntil(() =>
    bar(region, '[data-find-active]'),
  )) as HTMLElement
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
  expect(
    Math.abs(
      middle(active.getBoundingClientRect()) - (await lineMiddle(region, 4100)),
    ),
  ).toBeLessThan(1)
})

test('find scrolls to the current match and follows it', async ({
  onTestFinished,
}) => {
  const { region, update } = await renderViewer(
    { code: document_(300), find: { query: 'key-250:', active: 0 } },
    onTestFinished,
  )
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
  const box = region.getBoundingClientRect()
  const rect = bar(region, '[data-find-active]')!.getBoundingClientRect()
  expect(rect.top).toBeGreaterThanOrEqual(box.top)
  expect(rect.bottom).toBeLessThanOrEqual(box.bottom)

  update({ find: { query: 'key-2:', active: 0 } })
  await expect.poll(() => region.scrollTop).toBe(0)
})

test('a jump marks the line of a field and scrolls to it again on a new request', async ({
  onTestFinished,
}) => {
  const { region, update } = await renderViewer(
    {
      code: document_(300),
      jump: { field: 'key-200', start: 200, end: 200, nonce: 1 },
    },
    onTestFinished,
  )
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
  expect(bar(region, '[data-jump]')!.dataset.line).toBe('200')

  region.scrollTop = 0
  update({ jump: { field: 'key-200', start: 200, end: 200, nonce: 2 } })
  await expect.poll(() => region.scrollTop).toBeGreaterThan(0)
})

test('nothing is marked without find or jump, or when nothing matches', async ({
  onTestFinished,
}) => {
  const { region, update } = await renderViewer(
    { code: document_(20) },
    onTestFinished,
  )
  expect(region.querySelector('[data-find],[data-jump]')).toBeNull()
  update({ find: { query: 'nothing', active: 0 } })
  await new Promise((resolve) => setTimeout(resolve, 50))
  expect(region.querySelector('[data-find],[data-jump]')).toBeNull()
})

// The number a line shows: its marker's content, resolved from the line.
const shownNumber = (line: Element) => {
  const content = getComputedStyle(line, '::before').content
  const attribute = /^attr\(([\w-]+)\)$/.exec(content)?.[1]
  return attribute ? line.getAttribute(attribute) : content.replaceAll('"', '')
}

test('each line of a highlighted document shows its own number', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer({ code: document_(50) }, onTestFinished)
  await vi.waitUntil(() => region.querySelector('[data-highlighted]'))
  const lines = [...region.querySelectorAll('pre .line')]

  expect(lines.slice(0, 3).map(shownNumber)).toEqual(['1', '2', '3'])
  expect(lines.map((line) => line.textContent)).toEqual(
    lines.map((_, index) => `key-${index}: value`),
  )
})

test('a long document mounts only the lines in view, numbered as they are', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer(
    { code: document_(4001) },
    onTestFinished,
  )
  await vi.waitUntil(() => region.querySelector('pre .line'))
  expect(region.querySelectorAll('pre .line').length).toBeLessThan(100)

  region.scrollTop = region.scrollHeight
  const last = await vi.waitUntil(() =>
    region.querySelector('pre .line[data-line="4001"]'),
  )
  expect(shownNumber(last)).toBe('4001')
  expect(last.textContent).toBe('key-4000: value')
})
