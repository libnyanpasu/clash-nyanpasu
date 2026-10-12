// Layout uses the app's real Inter font, theme and Tailwind classes.
import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import FilterListRounded from '~icons/material-symbols/filter-list-rounded'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { Button } from '@nyanpasu/ui/button'
import { m } from '@/paraglide/messages'
import { getLocale, locales, setLocale } from '@/paraglide/runtime'
import { useDataPageBottomInset } from '../src/pages/(main)/main/_modules/data-page'
import { LogFileMenu } from '../src/pages/(main)/main/logs/_modules/log-file-menu'
import {
  LogClearButton,
  LogFollowButton,
  LogSearch,
} from '../src/pages/(main)/main/logs/_modules/log-viewer'
import { LogsLayout } from '../src/pages/(main)/main/logs/_modules/logs-layout'

const mocked = vi.hoisted(() => ({ navigate: vi.fn() }))

vi.mock('../src/pages/(main)/main/logs/route', () => ({
  Route: { useNavigate: () => mocked.navigate },
  LogLevelsLayout: ({ children }: { children: React.ReactNode }) => children,
}))

vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg width="24" height="24" {...props} />,
}))

const ARCHIVE_NAME = `${'application-log-'.repeat(12)}2026-10-13.log`

const WIDTHS = [208, 232, 240, 320, 896]

function LogRows() {
  const bottomInset = useDataPageBottomInset()

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 overflow-auto" data-testid="log-scroll">
        <div style={{ paddingBottom: bottomInset }}>
          {Array.from({ length: 40 }, (_, index) => (
            <div className="h-10" data-testid={`log-row-${index}`} key={index}>
              {m.logs_search_placeholder()} {index}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

async function mountLogsLayout() {
  const view = await render(
    <main className="flex h-full min-h-0 w-full" data-testid="frame">
      <LogsLayout
        source="app"
        search={
          <LogSearch
            value=""
            onChange={() => {}}
            placeholder={m.logs_filter_placeholder()}
          />
        }
        actions={
          <>
            <LogFileMenu
              file={null}
              files={[{ id: 'archive', name: ARCHIVE_NAME }]}
              onValueChange={() => {}}
            />
            <Button icon aria-label={m.logs_filters()} className="shrink-0">
              <FilterListRounded aria-hidden className="size-5" />
            </Button>
            <LogClearButton disabled={false} onClick={() => {}} />
          </>
        }
      >
        <LogRows />
      </LogsLayout>
    </main>,
  )
  onTestFinished(() => view.unmount())
  await document.fonts.ready

  const frame = view.getByTestId('frame').element() as HTMLElement
  frame.style.height = '640px'
  const page = frame.querySelector<HTMLElement>('[data-slot="data-page"]')!
  const dock = frame.querySelector<HTMLElement>('[data-slot="data-page-dock"]')!
  const surface = frame.querySelector('[data-slot="data-page-surface"]')!
  const toolbar = frame.querySelector('[data-slot="logs-toolbar"]')!
  const sourceTabs = view
    .getByRole('radiogroup', {
      name: m.logs_source_label(),
    })
    .element()
  const search = view.container.querySelector(
    '[data-slot="logs-search"]',
  )!.parentElement!
  const select = view
    .getByRole('button', {
      name: m.logs_file_label(),
    })
    .element()
  const filter = view.getByRole('button', { name: m.logs_filters() }).element()
  const clear = view
    .getByRole('button', { name: m.logs_clear_display() })
    .element()
  const scroll = view.getByTestId('log-scroll').element() as HTMLElement

  return {
    view,
    frame,
    page,
    dock,
    surface,
    toolbar,
    sourceTabs,
    search,
    select,
    filter,
    clear,
    scroll,
  }
}

for (const locale of locales) {
  test(`${locale}: log header, scroll surface and file toolbar fit light and dark layouts`, async () => {
    const previousLocale = getLocale()
    const hadDarkTheme = document.documentElement.classList.contains('dark')
    setLocale(locale, { reload: false })
    onTestFinished(() => {
      setLocale(previousLocale, { reload: false })
      document.documentElement.classList.toggle('dark', hadDarkTheme)
    })

    const {
      frame,
      page,
      dock,
      surface,
      toolbar,
      sourceTabs,
      search,
      select,
      filter,
      clear,
      scroll,
    } = await mountLogsLayout()

    for (const dark of [false, true]) {
      document.documentElement.classList.toggle('dark', dark)
      for (const width of WIDTHS) {
        frame.style.width = `${width}px`
        await expect.poll(() => frame.clientWidth).toBe(width)

        expect(page.getAttribute('aria-label')).toBe(m.navbar_label_logs())
        expect(frame.querySelector('[data-slot="data-page-header"]')).toBeNull()
        const surfaceBox = surface.getBoundingClientRect()
        const toolbarBox = toolbar.getBoundingClientRect()
        const dockBox = dock.getBoundingClientRect()
        expect(dock.contains(toolbar)).toBe(true)
        expect(toolbar.contains(sourceTabs)).toBe(true)
        expect(toolbar.contains(search)).toBe(true)
        expect(toolbar.contains(select)).toBe(true)
        expect(toolbar.contains(filter)).toBe(true)
        expect(toolbar.contains(clear)).toBe(true)
        expect(surfaceBox.top).toBeLessThanOrEqual(toolbarBox.top)
        expect(toolbarBox.top).toBeLessThan(surfaceBox.bottom)
        expect(toolbarBox.bottom).toBeLessThanOrEqual(dockBox.bottom)
        expect(dockBox.bottom).toBeLessThanOrEqual(
          page.getBoundingClientRect().bottom,
        )
        if (width === 896) expect(toolbarBox.height).toBe(44)
        else expect(toolbarBox.height).toBeGreaterThan(44)
        expect(toolbarBox.bottom).toBeLessThanOrEqual(
          frame.getBoundingClientRect().bottom,
        )
        expect(toolbar.scrollWidth).toBeLessThanOrEqual(toolbar.clientWidth)
        expect(search.getBoundingClientRect().width).toBeGreaterThanOrEqual(128)
        expect(scroll.scrollHeight).toBeGreaterThan(scroll.clientHeight)
        expect(scroll.getBoundingClientRect().bottom).toBeLessThanOrEqual(
          surfaceBox.bottom,
        )

        await expect
          .poll(() =>
            Number.parseFloat(
              getComputedStyle(scroll.firstElementChild!).paddingBottom,
            ),
          )
          .toBeGreaterThanOrEqual(toolbar.clientHeight + 40)
        scroll.scrollTop = scroll.scrollHeight
        scroll.dispatchEvent(new Event('scroll'))
        const lastRow = scroll.querySelector<HTMLElement>(
          '[data-testid="log-row-39"]',
        )!
        await expect
          .poll(() => lastRow.getBoundingClientRect().bottom)
          .toBeLessThanOrEqual(toolbarBox.top)

        const controls = [search, select, filter, clear].map((control) =>
          control.getBoundingClientRect(),
        )
        for (const control of controls) {
          expect(control.right).toBeLessThanOrEqual(toolbarBox.right)
          expect(control.left).toBeGreaterThanOrEqual(toolbarBox.left)
          expect(control.top).toBeGreaterThanOrEqual(toolbarBox.top)
          expect(control.bottom).toBeLessThanOrEqual(toolbarBox.bottom)
        }
        for (let index = 0; index < controls.length; index++) {
          for (const other of controls.slice(index + 1)) {
            const current = controls[index]
            expect(
              current.right <= other.left ||
                other.right <= current.left ||
                current.bottom <= other.top ||
                other.bottom <= current.top,
            ).toBe(true)
          }
        }
      }
    }
  })
}

test('file menu opens above the dock and wraps long file names within the viewport', async () => {
  const { view, frame, select } = await mountLogsLayout()
  frame.style.width = '240px'
  await view.getByRole('button', { name: m.logs_file_label() }).click()
  const menu = view.getByRole('menu', { name: m.logs_file_label() })
  await expect.element(menu).toBeVisible()
  await expect
    .element(view.getByRole('menuitemradio', { name: m.logs_current_file() }))
    .toHaveAttribute('aria-checked', 'true')
  const archive = view
    .getByRole('menuitemradio', { name: ARCHIVE_NAME })
    .element()
  expect(archive.scrollWidth).toBeLessThanOrEqual(archive.clientWidth)
  await expect
    .poll(() => menu.element().getBoundingClientRect().bottom)
    .toBeLessThanOrEqual(select.getBoundingClientRect().top - 7)
  const menuBox = menu.element().getBoundingClientRect()
  expect(menuBox.left).toBeGreaterThanOrEqual(8)
  expect(menuBox.right).toBeLessThanOrEqual(window.innerWidth - 8)
})

test('follow button uses a circular tonal surface and keeps the badge outside its ripple', async () => {
  const onClick = vi.fn()
  const view = await render(<LogFollowButton unseen={120} onClick={onClick} />)
  onTestFinished(() => view.unmount())
  const button = view.getByRole('button', { name: m.logs_follow_latest() })
  const style = getComputedStyle(button.element())
  expect(style.width).toBe('48px')
  expect(style.height).toBe('48px')
  expect(Number.parseFloat(style.borderRadius)).toBeGreaterThanOrEqual(24)
  expect(button.element().contains(view.getByText('99+').element())).toBe(false)
  await button.click()
  expect(onClick).toHaveBeenCalledOnce()
})
