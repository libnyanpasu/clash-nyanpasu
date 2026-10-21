import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { createRef } from 'react'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-react'
import {
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
} from '@nyanpasu/ui/navigation-list'

test('an item forwards its ref and data attributes and marks its state', async () => {
  const ref = createRef<HTMLButtonElement>()

  const view = await render(
    <NavigationList aria-label="Folders">
      <NavigationListGroup
        label="Folders"
        action={<button type="button">Add folder</button>}
      >
        <NavigationListItem ref={ref} active data-dragging={false}>
          <NavigationListItemIcon color="blue">i</NavigationListItemIcon>

          <NavigationListItemText headline="Work" supporting="3 profiles" />
        </NavigationListItem>

        <NavigationListItem active={false} data-drop-target>
          <NavigationListItemText headline="Home" supporting="1 profile" />
        </NavigationListItem>
      </NavigationListGroup>
    </NavigationList>,
  )

  const active = view.getByRole('button', { name: /Work/ })
  await expect.element(active).toHaveAttribute('data-active', 'true')
  await expect.element(active).toHaveAttribute('data-dragging', 'false')
  expect(ref.current).toBe(active.element())

  const target = view.getByRole('button', { name: /Home/ })
  await expect.element(target).toHaveAttribute('data-active', 'false')
  await expect.element(target).toHaveAttribute('data-drop-target', 'true')

  await expect
    .element(view.getByRole('group', { name: 'Folders' }))
    .toBeInTheDocument()
  await expect
    .element(view.getByRole('button', { name: 'Add folder' }))
    .toBeInTheDocument()
})
