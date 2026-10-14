// Layout uses the app theme and Tailwind classes with the real Sidebar provider.
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import ArticleRounded from '~icons/material-symbols/article-rounded'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@nyanpasu/ui/navigation-sidebar'
import { SearchField } from '@nyanpasu/ui/search-field'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'
import {
  DataPage,
  dataPageSearchVariants,
  DataPageToolbar,
} from '../src/pages/(main)/main/_modules/data-page'

vi.mock('./icon-stub', () => ({
  default: (props: object) => <svg {...props} />,
}))

function SelectedSidebarItem() {
  return (
    <NavigationSidebarItem
      data-testid="sidebar-item"
      active
      icon={<ArticleRounded />}
      renderLink={(content) => <a href="#">{content}</a>}
    >
      Profiles
    </NavigationSidebarItem>
  )
}

async function mountSidebar() {
  const view = await render(
    <SidebarProvider defaultOpen={false}>
      <div className="flex h-dvh w-dvw" data-testid="frame">
        <NavigationSidebar data-testid="sidebar">
          <NavigationSidebarContent className="min-h-0 flex-1">
            <SelectedSidebarItem />
          </NavigationSidebarContent>
          <NavigationSidebarToggle aria-label="Toggle sidebar">
            Collapse sidebar
          </NavigationSidebarToggle>
        </NavigationSidebar>
        <DataPage
          title="Example"
          toolbar={
            <DataPageToolbar>
              <SearchField
                className={dataPageSearchVariants({ className: 'flex-1' })}
                value=""
                onValueChange={() => {}}
                clearLabel="Clear"
                placeholder="Search"
              />
            </DataPageToolbar>
          }
        >
          <div />
        </DataPage>
      </div>
    </SidebarProvider>,
  )
  onTestFinished(() => view.unmount())

  const frame = view.getByTestId('frame').element() as HTMLElement
  const sidebar = view.getByTestId('sidebar').element() as HTMLElement
  const item = view.getByTestId('sidebar-item').element() as HTMLElement
  const icon = item.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar-icon"]',
  )!
  const label = item.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar-label"]',
  )!
  const toggle = sidebar.querySelectorAll('button')[0]!

  return { view, frame, sidebar, item, icon, label, toggle }
}

for (const dark of [false, true]) {
  test(`${dark ? 'dark' : 'light'} theme: collapsed rail labels and selected icon transition into the open sidebar`, async () => {
    const hadDarkTheme = document.documentElement.classList.contains('dark')
    document.documentElement.classList.toggle('dark', dark)
    await page.viewport(1280, 800)
    onTestFinished(async () => {
      document.documentElement.classList.toggle('dark', hadDarkTheme)
      await page.viewport(1280, 800)
    })

    const { frame, sidebar, item, icon, label, toggle } = await mountSidebar()
    const waitForFrames = async (count: number) => {
      for (let frame = 0; frame < count; frame++) {
        await new Promise<void>((resolve) => {
          requestAnimationFrame(() => resolve())
        })
      }
    }
    const labelOffset = () =>
      label.getBoundingClientRect().left - item.getBoundingClientRect().left

    await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(88)
    const expectFloatingToggle = () => {
      const sidebarBox = sidebar.getBoundingClientRect()
      const toggleBox = toggle.getBoundingClientRect()
      expect(getComputedStyle(toggle).position).toBe('absolute')
      const toolbarBox = frame
        .querySelector('[data-slot="data-page-toolbar"]')!
        .getBoundingClientRect()
      expect(toggleBox.height).toBe(44)
      expect(toolbarBox.height).toBe(44)
      expect(toggleBox.top).toBeCloseTo(toolbarBox.top, 0)
      expect(toggleBox.bottom).toBeCloseTo(toolbarBox.bottom, 0)
      expect(toggleBox.width).toBeCloseTo(sidebarBox.width - 16, 0)
      expect(sidebarBox.bottom - toggleBox.bottom).toBeCloseTo(8, 0)
      expect(toggleBox.left + toggleBox.width / 2).toBeCloseTo(
        sidebarBox.left + sidebarBox.width / 2,
        0,
      )
      const toggleLabel = toggle.querySelector<HTMLElement>(
        '[data-slot="navigation-sidebar-label"]',
      )!
      expect(toggleLabel.getAttribute('aria-hidden')).toBe(
        toggle.getAttribute('aria-expanded') === 'true' ? 'false' : 'true',
      )
      expect(toggleLabel.getBoundingClientRect().width > 0).toBe(
        toggle.getAttribute('aria-expanded') === 'true',
      )
      expect(toggle.getAttribute('aria-label')).toBe('Toggle sidebar')
    }
    expectFloatingToggle()
    const toggleIcon = toggle.querySelector<HTMLElement>(
      '[data-slot="navigation-sidebar-icon"]',
    )!
    expect(getComputedStyle(toggle).backgroundColor).toBe('rgba(0, 0, 0, 0)')
    expect(getComputedStyle(toggleIcon).backgroundColor).toBe(
      getComputedStyle(icon).backgroundColor,
    )
    expect(toggle.getBoundingClientRect().height).toBe(44)
    expect(frame.clientWidth).toBe(1280)
    expect(label.textContent).toBe('Profiles')
    expect(label.getBoundingClientRect().width).toBe(72)
    expect(icon.getBoundingClientRect().top).toBeLessThan(
      label.getBoundingClientRect().top,
    )
    expect(
      Math.abs(
        label.getBoundingClientRect().left +
          label.getBoundingClientRect().width / 2 -
          (item.getBoundingClientRect().left + item.clientWidth / 2),
      ),
    ).toBeLessThan(1)
    expect(getComputedStyle(item).backgroundColor).toBe('rgba(0, 0, 0, 0)')
    expect(getComputedStyle(icon).backgroundColor).not.toBe('rgba(0, 0, 0, 0)')

    const labelNode = label.firstChild
    const closedLeft = label.getBoundingClientRect().left
    toggle.click()
    await waitForFrames(4)
    const intermediateLeft = label.getBoundingClientRect().left
    expect(intermediateLeft).toBeGreaterThan(closedLeft + 1)
    expect(intermediateLeft).toBeLessThan(closedLeft + 51)
    expect(sidebar.getBoundingClientRect().width).toBeGreaterThan(88)
    expect(sidebar.getBoundingClientRect().width).toBeLessThan(200)

    await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(200)
    await expect.poll(labelOffset).toBeCloseTo(52, 0)
    await waitForFrames(4)
    expect(labelOffset()).toBeCloseTo(52, 0)
    expectFloatingToggle()
    expect(getComputedStyle(toggle).backgroundColor).toBe(
      getComputedStyle(item).backgroundColor,
    )
    const toggleLabel = toggle.querySelector(
      '[data-slot="navigation-sidebar-label"]',
    )!
    await expect
      .poll(
        () =>
          toggleLabel.getBoundingClientRect().left -
          toggle.getBoundingClientRect().left,
      )
      .toBeCloseTo(52, 0)
    expect(label.firstChild).toBe(labelNode)
    await expect
      .poll(() => label.getBoundingClientRect().width)
      .toBeCloseTo(item.getBoundingClientRect().width - 68, 0)
    await expect
      .poll(
        () =>
          label.getBoundingClientRect().left -
          item.getBoundingClientRect().left,
      )
      .toBeCloseTo(52, 0)
    expect(getComputedStyle(item).backgroundColor).not.toBe('rgba(0, 0, 0, 0)')
    expect(getComputedStyle(icon).backgroundColor).toBe('rgba(0, 0, 0, 0)')

    toggle.click()
    await expect
      .poll(() => {
        const left = label.getBoundingClientRect().left
        return left > closedLeft + 1 && left < closedLeft + 51
      })
      .toBe(true)
    await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(88)
    await expect
      .poll(() => label.getBoundingClientRect().left)
      .toBeCloseTo(closedLeft, 0)
    expect(label.firstChild).toBe(labelNode)
    expect(icon.getBoundingClientRect().bottom).toBeLessThan(
      label.getBoundingClientRect().top,
    )
  })
}
