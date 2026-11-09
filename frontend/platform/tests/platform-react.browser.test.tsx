import { expect, onTestFinished, test } from 'vitest'
import { render } from 'vitest-browser-react'
import { page } from 'vitest/browser'
import {
  Platform,
  PlatformLinux,
  PlatformProvider,
  PlatformWindows,
} from '../src/react'

test('platform gates use injected facts and do not mount excluded children', async ({
  onTestFinished,
}) => {
  const view = await render(
    <PlatformProvider platform={{ surface: 'browser', clientOs: 'windows' }}>
      <PlatformWindows>
        <span>desktop-only</span>
      </PlatformWindows>
      <Platform when={{ surface: 'browser', clientOs: 'windows' }}>
        <span>browser-windows</span>
      </Platform>
    </PlatformProvider>,
  )
  onTestFinished(() => view.unmount())
  await expect.element(page.getByText('browser-windows')).toBeInTheDocument()
  await expect.element(page.getByText('desktop-only')).not.toBeInTheDocument()
})

test.each([
  [{ surface: 'desktop', clientOs: 'linux' }, true],
  [{ surface: 'browser', clientOs: 'linux' }, false],
  [{ surface: 'desktop', clientOs: 'windows' }, false],
] as const)(
  'Linux platform gate mounts children only on a Linux desktop',
  async (platform, visible) => {
    const view = await render(
      <PlatformProvider platform={platform}>
        <PlatformLinux>
          <span>transparent proxy</span>
        </PlatformLinux>
      </PlatformProvider>,
    )
    onTestFinished(() => view.unmount())
    const content = page.getByText('transparent proxy')
    if (visible) {
      await expect.element(content).toBeInTheDocument()
    } else {
      await expect.element(content).not.toBeInTheDocument()
    }
  },
)
