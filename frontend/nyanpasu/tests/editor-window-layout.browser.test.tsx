import '@fontsource-variable/inter'
import '@nyanpasu/theme/styles/fonts.css'
import '@nyanpasu/theme/styles/theme.css'
import '@/assets/styles/tailwind.css'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-react'
import ActionButton from '../src/pages/(editor)/editor/_modules/action-button'
import { EditorFooter } from '../src/pages/(editor)/editor/_modules/footer'
import Header from '../src/pages/(editor)/editor/_modules/header'
import { EditorStatus } from '../src/pages/(editor)/editor/_modules/status'

vi.mock('@nyanpasu/platform', () => ({ createWindowAdapter: () => null }))
vi.mock('@nyanpasu/platform/react', () => ({
  PlatformDesktop: ({ children }: { children: React.ReactNode }) => children,
  PlatformMacos: ({ fallback }: { fallback: React.ReactNode }) => fallback,
}))
vi.mock('@nyanpasu/query', () => ({
  useSetting: () => ({ value: false, upsert: vi.fn() }),
}))
vi.mock('@/hooks/use-window-maximized', () => ({
  default: () => ({ isMaximized: false, toggleMaximize: vi.fn() }),
}))

test('editor chrome keeps long titles and action labels inside narrow windows', async ({
  onTestFinished,
}) => {
  const view = await render(
    <div style={{ width: 380 }} data-testid="window">
      <Header
        title="Material You 侧边栏（副本）very-long-filename.less"
        dirty
      />
      <EditorFooter>
        <EditorStatus editor={null} markers={[]} />
        <div className="flex-1" />
        <ActionButton>清除</ActionButton>
        <ActionButton>取消</ActionButton>
        <ActionButton variant="flat" className="px-5">
          应用
        </ActionButton>
        <ActionButton variant="flat" className="px-5">
          保存
        </ActionButton>
      </EditorFooter>
    </div>,
  )
  const fontSize = document.documentElement.style.fontSize
  onTestFinished(() => {
    document.documentElement.style.fontSize = fontSize
    return view.unmount()
  })

  const window = view.getByTestId('window').element() as HTMLElement
  const header = window.querySelector<HTMLElement>('[data-slot="app-header"]')!
  const title = window.querySelector<HTMLElement>(
    '[data-slot="app-header-logo-container"]',
  )!
  const reserved = header.querySelector<HTMLElement>('[aria-hidden="true"]')!
  const appName = window.querySelector<HTMLElement>(
    '[data-slot="editor-title-app"]',
  )!
  const footer = window.querySelector<HTMLElement>(
    '[data-slot="editor-footer-actions"]',
  )!

  for (const [width, rootFontSize] of [
    [240, 16],
    [320, 16],
    [380, 16],
    [580, 16],
    [800, 16],
    [380, 24],
    [580, 24],
  ]) {
    window.style.width = `${width}px`
    document.documentElement.style.fontSize = `${rootFontSize}px`
    expect(title.getBoundingClientRect().right).toBeLessThanOrEqual(
      reserved.getBoundingClientRect().left + 1,
    )
    expect(appName.getBoundingClientRect().right).toBeLessThanOrEqual(
      reserved.getBoundingClientRect().left + 1,
    )
    expect(footer.scrollWidth).toBeLessThanOrEqual(footer.clientWidth)
    for (const button of footer.querySelectorAll('button')) {
      expect(button.scrollWidth).toBeLessThanOrEqual(button.clientWidth)
      expect(button.scrollHeight).toBeLessThanOrEqual(button.clientHeight)
      expect(button.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        footer.getBoundingClientRect().bottom,
      )
    }
  }
})
