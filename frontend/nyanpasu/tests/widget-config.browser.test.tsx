import { createRoot } from 'react-dom/client'
import { beforeEach, expect, test, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { DndGridProvider } from '@nyanpasu/ui/dnd-grid'
import { TooltipProvider } from '@nyanpasu/ui/tooltip'
import {
  DashboardProvider,
  useDashboardContext,
  useWidgetConfig,
} from '@/pages/(main)/main/dashboard/_modules/provider'
import {
  WidgetId,
  type WidgetConfigStorage,
} from '@/pages/(main)/main/dashboard/_modules/widget-config'
import WidgetItem from '@/pages/(main)/main/dashboard/_modules/widget-item'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { DndContext } from '@dnd-kit/core'
import { RpcProvider } from '@nyanpasu/query/provider'

const backend = vi.hoisted(() => ({
  value: null as string | null,
  fail: false,
  writes: [] as string[],
  wait: null as Promise<void> | null,
}))
vi.mock('@/services/rpc', async (importOriginal) => {
  const original = await importOriginal<typeof import('@/services/rpc')>()
  return {
    ...original,
    rpc: {
      ...original.rpc,
      getStorageItem: async () => ({ status: 'ok', data: backend.value }),
      setStorageItem: async (_key: string, value: string) => {
        backend.writes.push(value)
        await backend.wait
        if (backend.fail) return { status: 'error', error: 'disk full' }
        backend.value = value
        return { status: 'ok', data: null }
      },
      listenResync: () => () => {},
      events: {
        ...original.rpc.events,
        storageValueChangedEvent: { listen: async () => () => {} },
      },
    },
  }
})

beforeEach(() => {
  localStorage.clear()
  backend.value = null
  backend.fail = false
  backend.writes = []
  backend.wait = null
})

function ExampleWidget({ id }: { id: string }) {
  const config = useWidgetConfig(id, WidgetId.ProxyShortcuts)
  return (
    <WidgetItem id={id} widgetType={WidgetId.ProxyShortcuts}>
      <div data-testid={`value-${id}`}>
        {config.orientation}/{config.buttons}/{config.order}
      </div>
    </WidgetItem>
  )
}

function mount(
  onTestFinished: (fn: () => void) => void,
  width = 6,
  nearBottom = false,
  overlay?: 'main' | 'sheet',
) {
  const container = document.createElement('div')
  document.body.append(container)
  const style = document.createElement('style')
  // Match the trigger and surface dimensions without the app's build plugins.
  style.textContent = `[data-slot="widget-config-trigger"] { position:absolute; bottom:-4px; left:50%; width:36px; height:24px; transform:translateX(-50%); }
    [data-slot="popover-content"] { width:288px; display:flex; flex-direction:column; overflow:hidden; border-radius:24px; background:white; border:1px solid #ccc; }
    [data-slot="popover-body"], [data-slot="widget-config-menu"], [data-slot="widget-config-scroll-area"] { display:flex; flex-direction:column; min-height:0; flex:1 1 auto; }
    [data-slot="widget-config-menu"] > h2 { flex-shrink:0; margin:0; padding:16px; font-size:14px; line-height:20px; }
    [data-slot="widget-config-fields"] { padding:0 16px 16px; }
    [data-slot="widget-config-fields"] > * + * { margin-top:16px; }
    [data-slot="widget-config-footer"] { flex-shrink:0; margin:0 16px; border-top:1px solid #ccc; padding:12px 0 16px; }
    [data-slot="scroll-area-viewport"] { display:flex; flex-direction:column; min-height:0; flex:1 1 auto; width:100%; }
    [data-slot="scroll-area-viewport"] > div { display:block !important; min-height:0; flex:1; }`
  document.head.append(style)
  const root = createRoot(container)
  const controls = {
    editing: (_value: boolean) => {},
    loading: true,
    dragStarts: 0,
  }
  const items = [
    { id: 'first', x: 0, y: 0, w: width, h: 2 },
    { id: 'second', x: 6, y: 0, w: 6, h: 2 },
  ]
  const Scene = () => {
    const { setIsEditing, isEditing, configLoading } = useDashboardContext()
    controls.editing = setIsEditing
    controls.loading = configLoading
    return (
      <DndGridProvider
        value={{
          displayItems: items,
          getItemRect: (item) => ({
            left: item.id === 'first' ? 40 : 450,
            top: nearBottom ? window.innerHeight - 170 : 40,
            width: 380,
            height: 150,
          }),
          dropInfoMap: {},
          activeItemId: null,
          resizingItemId: null,
          disabled: !isEditing,
          sourceOnly: overlay !== undefined,
          dragIdPrefix: overlay === 'sheet' ? 'sheet:' : '',
          isOverlay: overlay !== undefined,
          constraintsMapRef: { current: {} },
          onResizeStart: () => {},
          onResizeMove: () => {},
          onResizeEnd: () => {},
        }}
      >
        <ExampleWidget id="first" />
        <ExampleWidget id="second" />
      </DndGridProvider>
    )
  }
  root.render(
    <RpcProvider rpc={rpc}>
      <DashboardProvider>
        <TooltipProvider>
          <DndContext onDragStart={() => controls.dragStarts++}>
            <Scene />
          </DndContext>
        </TooltipProvider>
      </DashboardProvider>
    </RpcProvider>,
  )
  onTestFinished(() => {
    root.unmount()
    container.remove()
    style.remove()
  })
  return controls
}

const saved = () => JSON.parse(backend.value!) as WidgetConfigStorage

test('bottom popover preserves focus, stays open and saves independent instance options across remount', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect.poll(() => controls.loading).toBe(false)
  expect(
    document.querySelectorAll('[data-slot=widget-config-trigger]'),
  ).toHaveLength(0)
  controls.editing(true)
  const trigger = page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
  await trigger.click()
  const dialog = page.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  expect(dialog.element().getAttribute('data-side')).toBe('bottom')
  const triggerRect = trigger.element().getBoundingClientRect()
  expect(dialog.element().getBoundingClientRect().top).toBeGreaterThan(
    triggerRect.bottom,
  )
  await dialog
    .getByRole('radio', {
      name: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
    })
    .click()
  await expect
    .poll(() => saved().byInstance.first)
    .toMatchObject({ orientation: 'horizontal' })
  await expect
    .element(page.getByTestId('value-first'))
    .toHaveTextContent('horizontal/both/system-first')
  await expect
    .element(page.getByTestId('value-second'))
    .toHaveTextContent('vertical/both/system-first')
  await expect.element(dialog).toBeVisible()
  await userEvent.keyboard('{Escape}')
  await expect.element(dialog).not.toBeInTheDocument()
  await expect.poll(() => document.activeElement).toBe(trigger.element())
  const secondTrigger = page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(1)
  await secondTrigger.click()
  await page
    .getByRole('dialog')
    .getByRole('radio', {
      name: m.dashboard_widget_proxy_shortcuts_config_tun_first(),
    })
    .click()
  await expect
    .poll(() => saved().byInstance.second)
    .toMatchObject({ order: 'tun-first' })
  await userEvent.keyboard('{Escape}')
  expect(backend.writes).toHaveLength(2)
  // A new provider loads its authoritative backend config independently.
  const reloaded = mount(onTestFinished)
  await expect.poll(() => reloaded.loading).toBe(false)
  await expect
    .element(page.getByTestId('value-first').nth(1))
    .toHaveTextContent('horizontal/both/system-first')
  await expect
    .element(page.getByTestId('value-second').nth(1))
    .toHaveTextContent('vertical/both/tun-first')
})

test('one click on outside blank space closes the menu after editing an option', async ({
  onTestFinished,
}) => {
  const blank = document.createElement('div')
  blank.dataset.testid = 'outside-blank-space'
  blank.style.cssText =
    'position:fixed;right:16px;bottom:16px;width:80px;height:80px;'
  document.body.append(blank)
  onTestFinished(() => blank.remove())
  const controls = mount(onTestFinished)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  await page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
    .click()
  const dialog = page.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  await dialog.getByRole('radio', { name: 'TUN', exact: true }).click()
  await expect.element(dialog).toBeVisible()
  expect(controls.dragStarts).toBe(0)
  await page.getByTestId('outside-blank-space').click()
  await expect.element(dialog).not.toBeInTheDocument()
  await page.getByTestId('value-first').click()
  expect(controls.dragStarts).toBe(1)
})

test('failed autosave retains preview and retries the current options', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  await page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
    .click()
  const dialog = page.getByRole('dialog')
  backend.fail = true
  await dialog
    .getByRole('radio', {
      name: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
    })
    .click()
  await expect
    .element(dialog.getByText(m.dashboard_widget_config_save_failed()))
    .toBeVisible()
  await expect
    .element(page.getByTestId('value-first'))
    .toHaveTextContent('horizontal/both/system-first')
  backend.fail = false
  await dialog
    .getByRole('button', { name: m.dashboard_widget_config_retry() })
    .click()
  await expect
    .element(dialog.getByText(m.dashboard_widget_config_save_failed()))
    .not.toBeInTheDocument()
  expect(saved().byInstance.first).toMatchObject({ orientation: 'horizontal' })
  await dialog
    .getByRole('button', { name: m.dashboard_widget_config_reset() })
    .click()
  await expect
    .poll(() => saved().byInstance.first)
    .toMatchObject({ orientation: 'vertical' })
})

test('a narrow widget explains and disables horizontal layout for both buttons', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished, 3)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  await page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
    .click()
  const dialog = page.getByRole('dialog')
  await expect
    .element(
      dialog.getByText(
        m.dashboard_widget_proxy_shortcuts_config_widen({ columns: 6 }),
      ),
    )
    .toBeVisible()
  await expect
    .element(
      dialog.getByRole('radio', {
        name: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
      }),
    )
    .toBeDisabled()
  await dialog.getByRole('radio', { name: 'TUN', exact: true }).click()
  await expect
    .element(
      dialog.getByRole('radio', {
        name: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
      }),
    )
    .toBeEnabled()
})

test('a pending write prevents overlapping edits until its result arrives', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  await page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
    .click()
  const dialog = page.getByRole('dialog')
  let finish = () => {}
  backend.wait = new Promise<void>((resolve) => {
    finish = resolve
  })
  await dialog
    .getByRole('radio', {
      name: m.dashboard_widget_proxy_shortcuts_config_horizontal(),
    })
    .click()
  await expect.element(dialog.getByRole('status')).not.toBeInTheDocument()
  await expect
    .element(dialog.getByRole('radio', { name: 'TUN', exact: true }))
    .toBeDisabled()
  expect(backend.writes).toHaveLength(1)
  finish()
  await expect
    .element(dialog.getByRole('radio', { name: 'TUN', exact: true }))
    .toBeEnabled()
  await dialog.getByRole('radio', { name: 'TUN', exact: true }).click()
  await expect
    .poll(() => saved().byInstance.first)
    .toMatchObject({ orientation: 'horizontal', buttons: 'tun' })
})

test('popover flips above the trigger near the viewport bottom', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished, 6, true)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  const trigger = page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
  await trigger.click()
  const dialog = page.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  expect(dialog.element().getAttribute('data-side')).toBe('top')
  expect(dialog.element().getBoundingClientRect().bottom).toBeLessThan(
    trigger.element().getBoundingClientRect().top,
  )
})

test('only config fields scroll when the popover height is constrained', async ({
  onTestFinished,
}) => {
  const controls = mount(onTestFinished)
  await expect.poll(() => controls.loading).toBe(false)
  controls.editing(true)
  await page
    .getByRole('button', { name: m.dashboard_widget_config_title() })
    .nth(0)
    .click()
  const dialog = page.getByRole('dialog')
  await expect.element(dialog).toBeVisible()
  const viewport = dialog
    .element()
    .querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]')!
  await expect.poll(() => viewport.clientHeight).toBeGreaterThan(0)
  expect(viewport.scrollHeight).toBeLessThanOrEqual(viewport.clientHeight + 1)

  const constrained = document.createElement('style')
  constrained.textContent =
    '[data-slot="popover-content"] { max-height:200px !important; }'
  document.head.append(constrained)
  onTestFinished(() => constrained.remove())
  await expect
    .poll(() => viewport.scrollHeight - viewport.clientHeight)
    .toBeGreaterThan(0)
  const heading = dialog.getByRole('heading').element()
  const reset = dialog
    .getByRole('button', { name: m.dashboard_widget_config_reset() })
    .element()
  const headingTop = heading.getBoundingClientRect().top
  const resetTop = reset.getBoundingClientRect().top
  viewport.scrollTop = viewport.scrollHeight
  await expect.poll(() => viewport.scrollTop).toBeGreaterThan(0)
  expect(heading.getBoundingClientRect().top).toBe(headingTop)
  expect(reset.getBoundingClientRect().top).toBe(resetTop)
  expect(dialog.element().scrollTop).toBe(0)
  await expect.element(reset).toBeVisible()
})

test('drag overlays keep instance options while new-widget previews use defaults', async ({
  onTestFinished,
}) => {
  backend.value = JSON.stringify({
    version: 1,
    byInstance: {
      first: {
        type: 'proxy-shortcuts',
        orientation: 'horizontal',
        buttons: 'tun',
        order: 'system-first',
      },
    },
  })
  const main = mount(onTestFinished, 6, false, 'main')
  await expect.poll(() => main.loading).toBe(false)
  await expect
    .element(page.getByTestId('value-first').nth(0))
    .toHaveTextContent('horizontal/tun/system-first')
  const sheet = mount(onTestFinished, 6, false, 'sheet')
  await expect.poll(() => sheet.loading).toBe(false)
  await expect
    .element(page.getByTestId('value-first').nth(1))
    .toHaveTextContent('vertical/both/system-first')
  expect(
    document.querySelectorAll('[data-slot=widget-config-trigger]'),
  ).toHaveLength(0)
})
