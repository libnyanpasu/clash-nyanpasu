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
  NavigationListItemTrailing,
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

test('an item shows an image icon, fixed content before its text and a trailing slot', async () => {
  const view = await render(
    <NavigationList aria-label="Groups">
      <NavigationListGroup label="Groups">
        <NavigationListItem active={false}>
          <NavigationListItemIcon image>
            <img alt="" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" />
          </NavigationListItemIcon>

          <NavigationListItemText
            headline="Proxy"
            supporting="JP-02"
            supportingStart={<span>Select</span>}
          />

          <NavigationListItemTrailing>54 ms</NavigationListItemTrailing>
        </NavigationListItem>
      </NavigationListGroup>
    </NavigationList>,
  )

  const item = view.getByRole('button', { name: /Proxy/ })
  const text = item.element().textContent
  expect(text).toContain('Select')
  expect(text).toContain('JP-02')
  expect(text).toContain('54 ms')

  // The picture is shown as it is: no shape around it, no background and no
  // clipping, so nothing of it is cropped or tinted.
  const icon = item
    .element()
    .querySelector('[data-slot="navigation-list-item-icon"]')
  expect(icon?.querySelector('img')).not.toBeNull()
  expect(icon?.querySelector('[data-slot="material-shape"]')).toBeNull()

  for (const element of [icon, icon?.querySelector('img')]) {
    const style = getComputedStyle(element as Element)
    expect(style.backgroundColor).toBe('rgba(0, 0, 0, 0)')
    expect(style.clipPath).toBe('none')
    expect(style.borderRadius).toBe('0px')
  }
  expect(getComputedStyle(icon as Element).overflow).toBe('visible')
  expect(getComputedStyle(icon!.querySelector('img')!).objectFit).toBe(
    'contain',
  )
})
