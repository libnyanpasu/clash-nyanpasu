import { afterEach, beforeEach, expect, test, vi } from 'vitest'

const platform = vi.hoisted(() => ({ tauri: false }))
const createEditorWindow = vi.hoisted(() => vi.fn(async () => null))
const closeNative = vi.hoisted(() => vi.fn(async () => {}))

vi.mock('@nyanpasu/platform', () => ({ isTauri: () => platform.tauri }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ close: closeNative }),
}))
vi.mock('@/services/rpc', () => ({ rpc: { createEditorWindow } }))

const { closeEditorWindow, openEditorWindow } =
  await import('../src/utils/editor-window.ts')

const open = vi.fn()
const close = vi.fn()

beforeEach(() => {
  platform.tauri = false
  vi.stubGlobal('window', { open, close })
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

test('the desktop app opens editors as native windows', async () => {
  platform.tauri = true

  await openEditorWindow('profile', 'dns')

  expect(createEditorWindow).toHaveBeenCalledWith('profile', 'dns')
  expect(open).not.toHaveBeenCalled()
})

test('a browser opens the editor routes in a new window', async () => {
  await openEditorWindow('profile', 'a b')
  await openEditorWindow('css-editor', null)

  expect(createEditorWindow).not.toHaveBeenCalled()
  expect(open.mock.calls).toEqual([
    ['/editor/profile?uid=a%20b', '_blank'],
    ['/editor/css', '_blank'],
  ])
})

test('an editor closes the browser window it runs in', async () => {
  await closeEditorWindow()

  expect(close).toHaveBeenCalledOnce()
})

test('an editor closes its native window on the desktop', async () => {
  platform.tauri = true

  await closeEditorWindow()

  expect(closeNative).toHaveBeenCalledOnce()
  expect(close).not.toHaveBeenCalled()
})
