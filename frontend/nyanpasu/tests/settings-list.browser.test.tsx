import '../../theme/src/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { createRef } from 'react'
import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { Button } from '@nyanpasu/ui/button'
import { EmptyState } from '@nyanpasu/ui/empty-state'
import {
  SettingsCard,
  SettingsCardContent,
} from '../src/pages/(main)/main/_modules/settings-card'
import {
  SettingsList,
  SettingsListActions,
  SettingsListContent,
  SettingsListItem,
} from '../src/pages/(main)/main/_modules/settings-list'

test('empty lists preserve the Web UI panel size and put adding below at the right', async ({
  onTestFinished,
}) => {
  const view = await render(
    <div className="w-96 max-w-full">
      <SettingsCard data-testid="original">
        <SettingsCardContent>
          <EmptyState
            tone="normal"
            className="min-h-40"
            message="没有找到记录，尝试添加一个吧"
          />
        </SettingsCardContent>
      </SettingsCard>

      <SettingsList>
        <SettingsListContent
          empty
          emptyMessage="没有找到记录，尝试添加一个吧"
          data-testid="content"
        />
        <SettingsListActions>
          <Button variant="raised">添加</Button>
        </SettingsListActions>
      </SettingsList>
    </div>,
  )
  onTestFinished(() => view.unmount())

  const original = view
    .getByTestId('original')
    .element()
    .getBoundingClientRect()
  const content = view.getByTestId('content').element().getBoundingClientRect()
  const add = view
    .getByRole('button', { name: '添加' })
    .element()
    .getBoundingClientRect()

  expect(content.height).toBe(original.height)
  expect(content.width).toBe(original.width)
  expect(add.top).toBeGreaterThan(content.bottom)
  expect(add.right).toBe(content.right)
})

test.each([1, 3])(
  'a list with %i rows joins its explanation as the final rounded item',
  async (count) => {
    const ref = createRef<HTMLDivElement>()
    const view = await render(
      <SettingsList className="w-80 max-w-full">
        <SettingsListContent
          empty={false}
          data-testid="content"
          description={<p>说明文本单独占一项。</p>}
        >
          {Array.from({ length: count }, (_, index) => (
            <SettingsListItem key={index} ref={index === 0 ? ref : undefined}>
              <p className="min-w-0 flex-1 [overflow-wrap:anywhere]">
                stun.example.com:3478
              </p>
              <Button icon aria-label={`编辑 ${index}`}>
                …
              </Button>
            </SettingsListItem>
          ))}
        </SettingsListContent>
      </SettingsList>,
    )
    onTestFinished(() => view.unmount())

    const group = view.getByTestId('content').element()
    const rows = Array.from(group.children)
    expect(rows).toHaveLength(count + 1)
    expect(ref.current).toBe(rows[0])
    expect(rows.at(-1)).toHaveTextContent('说明文本单独占一项。')

    const first = getComputedStyle(rows[0])
    const last = getComputedStyle(rows.at(-1)!)
    expect(parseFloat(first.borderTopLeftRadius)).toBeGreaterThan(
      parseFloat(first.borderBottomLeftRadius),
    )
    expect(last.borderTopLeftRadius).toBe(first.borderBottomLeftRadius)
    expect(last.borderBottomLeftRadius).toBe(first.borderTopLeftRadius)
    for (const row of rows.slice(1, -1)) {
      const style = getComputedStyle(row)
      expect(style.borderTopLeftRadius).toBe(first.borderBottomLeftRadius)
      expect(style.borderBottomLeftRadius).toBe(first.borderBottomLeftRadius)
    }
    expect(group.scrollWidth).toBeLessThanOrEqual(group.clientWidth)
  },
)
