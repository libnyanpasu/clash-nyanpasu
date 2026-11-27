import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, test, vi } from 'vitest'
import {
  Item,
  ItemAction,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemSection,
  ItemSectionHeader,
  ItemSectionTitle,
  ItemSplitSwitch,
  ItemSwitch,
  ItemTitle,
} from '@nyanpasu/ui/item-group'

function mount(
  element: React.ReactNode,
  onTestFinished: (fn: () => void) => void,
) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)
  onTestFinished(() => {
    root.unmount()
    container.remove()
  })
  root.render(element)
  return container
}

test('ItemAction fires onClick when enabled and blocks when disabled', async ({
  onTestFinished,
}) => {
  const onActive = vi.fn()
  const onDisabled = vi.fn()
  const onDisabledLink = vi.fn((event: React.MouseEvent) => {
    event.preventDefault()
  })

  const container = mount(
    <ItemSection>
      <ItemSectionHeader>
        <ItemSectionTitle>Network</ItemSectionTitle>
      </ItemSectionHeader>
      <ItemGroup>
        <ItemAction
          title="Active entry"
          description="Opens details"
          value="Enabled"
          selected
          onClick={onActive}
        />
        <ItemAction
          title="Disabled entry"
          description="Not available"
          disabled
          onClick={onDisabled}
        />
        <ItemAction
          asChild
          title="Disabled link"
          disabled
          onClick={onDisabledLink}
        >
          <a href="#blocked" />
        </ItemAction>
      </ItemGroup>
    </ItemSection>,
    onTestFinished,
  )

  await expect
    .poll(() => container.querySelectorAll('[data-slot="item-action"]').length)
    .toBe(3)

  const [activeButton, disabledButton, disabledLink] = Array.from(
    container.querySelectorAll<HTMLElement>('[data-slot="item-action"]'),
  )

  expect(activeButton.getAttribute('aria-current')).toBe('true')
  expect(activeButton.hasAttribute('data-selected')).toBe(true)
  activeButton.click()
  expect(onActive).toHaveBeenCalledTimes(1)

  expect((disabledButton as HTMLButtonElement).disabled).toBe(true)
  disabledButton.click()
  expect(onDisabled).not.toHaveBeenCalled()

  expect(disabledLink.getAttribute('aria-disabled')).toBe('true')
  expect(disabledLink.getAttribute('tabindex')).toBe('-1')
  disabledLink.click()
  expect(onDisabledLink).not.toHaveBeenCalled()
})

test('ItemSwitch toggles via both switch control and row label', async ({
  onTestFinished,
}) => {
  const onChange = vi.fn()

  function Controlled() {
    const [checked, setChecked] = useState(false)

    return (
      <ItemGroup>
        <ItemSwitch
          title="Auto launch"
          description="Start automatically on login"
          checked={checked}
          onCheckedChange={(next) => {
            setChecked(next)
            onChange(next)
          }}
        />
        <ItemSwitch title="Locked switch" checked={false} disabled />
      </ItemGroup>
    )
  }

  const container = mount(<Controlled />, onTestFinished)

  await expect
    .poll(() => container.querySelectorAll('[data-slot="item-switch"]').length)
    .toBe(2)

  const [firstRow, disabledRow] = Array.from(
    container.querySelectorAll<HTMLElement>('[data-slot="item-switch"]'),
  )
  const label = firstRow.querySelector<HTMLLabelElement>('label')!
  const switchBtn = firstRow.querySelector<HTMLButtonElement>(
    '[data-slot="switch"]',
  )!

  expect(switchBtn.getAttribute('aria-labelledby')).toBeTruthy()
  expect(switchBtn.getAttribute('aria-describedby')).toBeTruthy()

  label.click()
  await expect.poll(() => switchBtn.getAttribute('data-state')).toBe('checked')
  expect(onChange).toHaveBeenLastCalledWith(true)

  switchBtn.click()
  await expect
    .poll(() => switchBtn.getAttribute('data-state'))
    .toBe('unchecked')
  expect(onChange).toHaveBeenLastCalledWith(false)

  const disabledSwitch = disabledRow.querySelector<HTMLButtonElement>(
    '[data-slot="switch"]',
  )!
  expect(disabledSwitch.disabled).toBe(true)
})

test('ItemSplitSwitch blocks detail action when unchecked and enables it when checked', async ({
  onTestFinished,
}) => {
  const onAction = vi.fn()
  const onLinkAction = vi.fn((event: React.MouseEvent) => {
    event.preventDefault()
  })

  function Controlled() {
    const [enabled, setEnabled] = useState(false)

    return (
      <ItemGroup>
        <ItemSplitSwitch
          title="Proxy guard"
          description="Keep system proxy settings enforced"
          checked={enabled}
          onCheckedChange={setEnabled}
          onAction={onAction}
        />
        <ItemSplitSwitch
          title="Network rules"
          description="Automatic profile switching"
          checked={enabled}
          onCheckedChange={setEnabled}
          actionAsChild
          onAction={onLinkAction}
        >
          <a href="#rules" />
        </ItemSplitSwitch>
        <Item>
          <ItemContent>
            <ItemTitle>Static item</ItemTitle>
            <ItemDescription>Non-interactive info row</ItemDescription>
          </ItemContent>
        </Item>
      </ItemGroup>
    )
  }

  const container = mount(<Controlled />, onTestFinished)

  await expect
    .poll(
      () =>
        container.querySelectorAll('[data-slot="item-split-switch"]').length,
    )
    .toBe(2)

  const [buttonRow, linkRow] = Array.from(
    container.querySelectorAll<HTMLElement>('[data-slot="item-split-switch"]'),
  )

  const actionButton = buttonRow.querySelector<HTMLButtonElement>(
    '[data-slot="item-split-switch-action"]',
  )!
  const actionLink = linkRow.querySelector<HTMLAnchorElement>(
    '[data-slot="item-split-switch-action"]',
  )!
  const switchControl = buttonRow.querySelector<HTMLButtonElement>(
    '[data-slot="switch"]',
  )!
  const switchArea = buttonRow.querySelector<HTMLElement>(
    '[data-slot="item-split-switch-control"]',
  )!

  // While unchecked, detail targets and the surrounding switch padding are inert;
  // only the Switch button itself can be clicked to enable.
  expect(actionButton.disabled).toBe(true)
  expect(actionLink.getAttribute('aria-disabled')).toBe('true')
  actionButton.click()
  actionLink.click()
  switchArea.click()
  expect(onAction).not.toHaveBeenCalled()
  expect(onLinkAction).not.toHaveBeenCalled()
  expect(switchControl.getAttribute('data-state')).toBe('unchecked')

  // Turn the switch on -> detail targets become interactive.
  switchControl.click()
  await expect.poll(() => actionButton.disabled).toBe(false)
  expect(actionLink.getAttribute('aria-disabled')).toBeNull()

  actionButton.click()
  actionLink.click()
  expect(onAction).toHaveBeenCalledTimes(1)
  expect(onLinkAction).toHaveBeenCalledTimes(1)

  // When enabled, clicking the blank area around the switch also triggers the
  // row's detail action without toggling the switch off.
  switchArea.click()
  expect(onAction).toHaveBeenCalledTimes(2)
  expect(switchControl.getAttribute('data-state')).toBe('checked')
})

test('disabled ItemSplitSwitch blocks both detail action and switch toggle', async ({
  onTestFinished,
}) => {
  const onAction = vi.fn()
  const onCheckedChange = vi.fn()

  const container = mount(
    <ItemGroup>
      <ItemSplitSwitch
        title="Disabled feature"
        description="Unavailable in current mode"
        checked
        disabled
        onCheckedChange={onCheckedChange}
        onAction={onAction}
      />
    </ItemGroup>,
    onTestFinished,
  )

  await expect
    .poll(() => container.querySelector('[data-slot="item-split-switch"]'))
    .not.toBeNull()

  const row = container.querySelector<HTMLElement>(
    '[data-slot="item-split-switch"]',
  )!
  const actionButton = row.querySelector<HTMLButtonElement>(
    '[data-slot="item-split-switch-action"]',
  )!
  const switchControl = row.querySelector<HTMLButtonElement>(
    '[data-slot="switch"]',
  )!
  const switchArea = row.querySelector<HTMLElement>(
    '[data-slot="item-split-switch-control"]',
  )!

  expect(actionButton.disabled).toBe(true)
  expect(switchControl.disabled).toBe(true)

  actionButton.click()
  switchArea.click()
  switchControl.click()

  expect(onAction).not.toHaveBeenCalled()
  expect(onCheckedChange).not.toHaveBeenCalled()
})
