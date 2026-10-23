import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import ArticleRounded from '~icons/material-symbols/article-rounded'
import { memo, Profiler, useState, type MouseEvent } from 'react'
import { expect, onTestFinished, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import { page, userEvent } from 'vitest/browser'
import {
  NavigationSidebar,
  NavigationSidebarItem,
} from '@nyanpasu/ui/navigation-sidebar'
import {
  SidebarProvider,
  SidebarToggleButton,
} from '@nyanpasu/ui/slider-sidebar'

async function mountSidebar() {
  const onNavigate = vi.fn()
  const onClick = vi.fn((event: MouseEvent<HTMLAnchorElement>) =>
    event.preventDefault(),
  )
  const view = await render(
    <SidebarProvider defaultOpen={false}>
      <div className="flex h-dvh w-dvw" data-testid="frame">
        <NavigationSidebar
          data-testid="navigation-sidebar"
          className="my-2 ml-2 max-md:inset-y-2 max-md:left-2 max-md:h-auto md:my-3 md:ml-3"
        >
          <div className="flex min-h-0 flex-1 flex-col gap-1 p-2">
            <NavigationSidebarItem
              active
              data-testid="sidebar-item"
              icon={<ArticleRounded />}
              onClick={onNavigate}
              renderLink={(content) => (
                <a href="#item" onClick={onClick}>
                  {content}
                </a>
              )}
            >
              Profiles
            </NavigationSidebarItem>
          </div>
          <div className="flex h-16 shrink-0 items-center justify-end p-2">
            <SidebarToggleButton />
          </div>
        </NavigationSidebar>
      </div>
    </SidebarProvider>,
  )
  onTestFinished(() => view.unmount())

  const sidebar = view
    .getByTestId('navigation-sidebar')
    .element() as HTMLElement
  const item = view.getByTestId('sidebar-item').element() as HTMLElement
  const capsule = item.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar-icon"]',
  )!
  const label = item.querySelector<HTMLElement>(
    '[data-slot="navigation-sidebar-label"]',
  )!
  const toggle = sidebar.querySelectorAll('button')[0]!

  // Capture transient ripples before a slow provider round trip can outlast
  // their animation. Keep geometry and hosts from insertion time after removal.
  const ripples: {
    parent: HTMLElement | null
    offsetParent: Element | null
    left: string
    top: string
    width: string
  }[] = []
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (
          node instanceof HTMLElement &&
          node.dataset.slot === 'ripple-item'
        ) {
          ripples.push({
            parent: node.parentElement,
            offsetParent: node.offsetParent,
            left: node.style.left,
            top: node.style.top,
            width: node.style.width,
          })
        }
      }
    }
  })
  observer.observe(item, { childList: true, subtree: true })
  onTestFinished(() => observer.disconnect())

  return {
    view,
    sidebar,
    item,
    capsule,
    label,
    toggle,
    onClick,
    onNavigate,
    ripples,
  }
}

test('collapsed clicks ripple inside the active icon capsule', async () => {
  const { capsule, onClick, onNavigate, ripples } = await mountSidebar()
  await expect.poll(() => capsule.getBoundingClientRect().width).toBe(56)
  await expect.poll(() => capsule.getBoundingClientRect().height).toBe(32)
  expect(getComputedStyle(capsule).overflow).toBe('hidden')
  expect(getComputedStyle(capsule).backgroundColor).not.toBe('rgba(0, 0, 0, 0)')

  await userEvent.click(page.getByTestId('sidebar-item'))
  await expect.poll(() => ripples).toHaveLength(1)
  expect(ripples[0].parent).toBe(capsule)
  expect(onClick).toHaveBeenCalledTimes(1)
  expect(onNavigate).toHaveBeenCalledTimes(1)
})

test('collapsed label area and keyboard activation center the capsule ripple', async () => {
  const { item, capsule, ripples } = await mountSidebar()
  await expect.poll(() => item.getBoundingClientRect().height).toBe(72)

  const rect = item.getBoundingClientRect()
  item.dispatchEvent(
    new MouseEvent('click', {
      bubbles: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.bottom - 1,
      detail: 1,
    }),
  )
  await expect.poll(() => ripples).toHaveLength(1)
  expect(ripples[0].parent).toBe(capsule)
  expect(ripples[0].left).toBe('0px')
  expect(ripples[0].top).toBe('-12px')

  // Each ripple expires independently; keyboard input need not overlap the
  // earlier pointer animation to prove it uses the same capsule coordinates.
  await expect
    .poll(() => capsule.querySelectorAll('[data-slot="ripple-item"]'))
    .toHaveLength(0)
  await userEvent.tab()
  await expect.element(page.getByTestId('sidebar-item')).toHaveFocus()
  await userEvent.keyboard('{Enter}')
  // Assert after expiry too, as can happen when CI resumes from keyboard input.
  await expect
    .poll(() => capsule.querySelectorAll('[data-slot="ripple-item"]'))
    .toHaveLength(0)
  await expect.poll(() => ripples).toHaveLength(2)
  expect(ripples[1].parent).toBe(capsule)
  expect(ripples[1].left).toBe('0px')
  expect(ripples[1].top).toBe('-12px')
})

test('expanded clicks ripple across the row and expose keyboard focus', async () => {
  const { sidebar, item, capsule, toggle, ripples } = await mountSidebar()
  await userEvent.click(toggle)
  await expect.poll(() => sidebar.getBoundingClientRect().width).toBe(200)
  await expect.poll(() => item.getBoundingClientRect().height).toBe(44)

  expect(item.className).toContain('focus-visible:ring-2')
  await userEvent.click(page.getByTestId('sidebar-item'))
  await expect.poll(() => ripples).toHaveLength(1)
  const ripple = ripples[0]
  // Button's display: contents wrapper preserves the row as the positioning host.
  expect(ripple.offsetParent).toBe(item)
  expect(Number.parseFloat(ripple.width)).toBe(item.clientWidth)
  expect(capsule.querySelector('[data-slot="ripple-item"]')).toBeNull()

  await userEvent.keyboard('{Enter}')
  await expect.poll(() => ripples).toHaveLength(2)
  const keyboardRipple = ripples[1]
  const itemRect = item.getBoundingClientRect()
  const rippleSize = Number.parseFloat(keyboardRipple.width)
  expect(keyboardRipple.left).toBe(`${(itemRect.width - rippleSize) / 2}px`)
  expect(keyboardRipple.top).toBe(`${(itemRect.height - rippleSize) / 2}px`)
})

test('an item does not render again with the page around the sidebar', async () => {
  let itemRenders = 0
  let renderPage = () => {}
  // An entry that renders only with its own props, as a page keeps its
  // entries while the sidebar around them renders.
  const Entry = memo(function Entry() {
    return (
      <Profiler id="item" onRender={() => (itemRenders += 1)}>
        <NavigationSidebarItem active={false} icon={<ArticleRounded />}>
          Profiles
        </NavigationSidebarItem>
      </Profiler>
    )
  })
  function Page() {
    const [count, setCount] = useState(0)
    renderPage = () => setCount((value) => value + 1)

    return (
      <SidebarProvider defaultOpen={false}>
        <span data-testid="count">{count}</span>
        <NavigationSidebar>
          <Entry />
        </NavigationSidebar>
      </SidebarProvider>
    )
  }
  const view = await render(<Page />)
  onTestFinished(() => view.unmount())
  await expect.element(view.getByTestId('count')).toHaveTextContent('0')
  const before = itemRenders

  renderPage()

  await expect.element(view.getByTestId('count')).toHaveTextContent('1')
  expect(itemRenders).toBe(before)
})
