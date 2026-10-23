import { afterEach, expect, it, vi } from 'vitest'
import { probeWebviewCapabilities } from '@/utils/webview-capabilities'

afterEach(() => vi.unstubAllGlobals())

it('returns partial data when graphics and user agent hints are missing', async () => {
  vi.stubGlobal('navigator', { userAgent: 'Minimal browser' })
  vi.stubGlobal('window', { devicePixelRatio: 2 })
  vi.stubGlobal('document', undefined)

  expect(await probeWebviewCapabilities()).toEqual({
    userAgent: 'Minimal browser',
    devicePixelRatio: 2,
    webgpu: { available: false },
  })
})

it('returns an empty result when browser globals are absent', async () => {
  vi.stubGlobal('navigator', undefined)
  vi.stubGlobal('window', undefined)
  vi.stubGlobal('document', undefined)

  expect(await probeWebviewCapabilities()).toEqual({})
})

it('collects hints and adapter details, falls back to WebGL and releases it', async () => {
  const loseContext = vi.fn()
  const getContext = vi.fn((kind: string) =>
    kind === 'webgl2'
      ? null
      : {
          getExtension: (name: string) =>
            name === 'WEBGL_debug_renderer_info'
              ? { UNMASKED_VENDOR_WEBGL: 1, UNMASKED_RENDERER_WEBGL: 2 }
              : { loseContext },
          getParameter: (key: number) => (key === 1 ? 'vendor' : 'renderer'),
        },
  )
  const getHighEntropyValues = vi.fn().mockResolvedValue({
    fullVersionList: [{ brand: 'Browser', version: '1.2' }],
    platformVersion: '3.4',
  })
  vi.stubGlobal('navigator', {
    userAgent: 'Test',
    userAgentData: { getHighEntropyValues },
    gpu: { requestAdapter: async () => ({ info: { vendor: 'GPU vendor' } }) },
  })
  vi.stubGlobal('window', { devicePixelRatio: 1 })
  vi.stubGlobal('document', { createElement: () => ({ getContext }) })

  const page = await probeWebviewCapabilities()

  expect(page.fullVersionList).toEqual([{ brand: 'Browser', version: '1.2' }])
  expect(page.platformVersion).toBe('3.4')
  expect(page.webgl).toEqual({ vendor: 'vendor', renderer: 'renderer' })
  expect(page.webgpu?.adapter?.vendor).toBe('GPU vendor')
  expect(getHighEntropyValues).toHaveBeenCalledWith([
    'fullVersionList',
    'platformVersion',
  ])
  expect(getContext.mock.calls).toEqual([['webgl2'], ['webgl']])
  expect(loseContext).toHaveBeenCalledOnce()
})

it('keeps basic facts when optional APIs fail and still releases WebGL', async () => {
  const loseContext = vi.fn()
  vi.stubGlobal('navigator', {
    userAgent: 'Test',
    userAgentData: {
      getHighEntropyValues: async () => {
        throw new Error('denied')
      },
    },
    gpu: {
      requestAdapter: async () => {
        throw new Error('unavailable')
      },
    },
  })
  vi.stubGlobal('window', { devicePixelRatio: 1 })
  vi.stubGlobal('document', {
    createElement: () => ({
      getContext: () => ({
        getExtension: (name: string) => {
          if (name === 'WEBGL_debug_renderer_info') throw new Error('denied')
          return { loseContext }
        },
      }),
    }),
  })

  expect(await probeWebviewCapabilities()).toEqual({
    userAgent: 'Test',
    devicePixelRatio: 1,
    webgpu: { available: true },
  })
  expect(loseContext).toHaveBeenCalledOnce()
})
