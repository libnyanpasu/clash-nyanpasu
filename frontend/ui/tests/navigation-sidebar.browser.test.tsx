import ArticleRounded from '~icons/material-symbols/article-rounded'
import { memo, Profiler, useState } from 'react'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  NavigationSidebar,
  NavigationSidebarItem,
} from '@nyanpasu/ui/navigation-sidebar'
import { SidebarProvider } from '@nyanpasu/ui/slider-sidebar'

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
