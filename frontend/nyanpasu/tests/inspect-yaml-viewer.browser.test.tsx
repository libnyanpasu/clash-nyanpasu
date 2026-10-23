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

/** The vertical middle of a line of text, measured from the rendered text. */
function lineMiddle(region: HTMLElement, line: number) {
  return middle(
    region.querySelectorAll('pre .line')[line].getBoundingClientRect(),
  )
}

test('a large document is shown as plain text', async ({ onTestFinished }) => {
  const { container, region } = await renderViewer(
    { code: document_(4001) },
    onTestFinished,
  )
  await vi.waitUntil(() => region.querySelector('pre code'))
  // Highlighting is not even attempted past the cap.
  await new Promise((resolve) => setTimeout(resolve, 300))
  expect(container.querySelector('.shiki')).toBeNull()
  expect(region.querySelector('pre code')?.textContent).toContain('key-4000')
})

test('a small document is highlighted', async ({ onTestFinished }) => {
  const { container } = await renderViewer(
    { code: document_(50) },
    onTestFinished,
  )
  await vi.waitUntil(() => container.querySelector('.shiki'))
})

test('find marks the matching lines at the lines they are on', async ({
  onTestFinished,
}) => {
  const { region } = await renderViewer(
    { code: document_(60), find: { query: 'key-4', active: 3 } },
    onTestFinished,
  )
  await vi.waitUntil(() => region.querySelector('.shiki'))

  // key-4 and key-40 .. key-49.
  expect(region.querySelectorAll('[data-find]')).toHaveLength(11)
  const active = bar(region, '[data-find-active]')!
  // The fourth match is key-42, on the 43rd line.
  expect(active.dataset.line).toBe('42')
  expect(
    Math.abs(middle(active.getBoundingClientRect()) - lineMiddle(region, 42)),
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
    Math.abs(middle(active.getBoundingClientRect()) - lineMiddle(region, 4100)),
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
