import type { PageCapabilities } from '@nyanpasu/rpc/types'

type WebGpuAdapterInfo = {
  vendor?: string
  architecture?: string
  device?: string
  description?: string
}

// These optional APIs are not available in every webview's DOM declarations.
type CapabilityNavigator = Navigator & {
  userAgentData?: {
    getHighEntropyValues(hints: string[]): Promise<{
      fullVersionList?: { brand: string; version: string }[]
      platformVersion?: string
    }>
  }
  gpu?: {
    requestAdapter(): Promise<{ info?: WebGpuAdapterInfo } | null>
  }
}

function probeWebGl(): PageCapabilities['webgl'] {
  let gl: WebGLRenderingContext | WebGL2RenderingContext | null = null
  try {
    const canvas = document.createElement('canvas')
    gl = canvas.getContext('webgl2') ?? canvas.getContext('webgl')
    if (!gl) return

    const debug = gl.getExtension('WEBGL_debug_renderer_info')
    if (!debug) return

    return {
      vendor: gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) as string,
      renderer: gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) as string,
    }
  } catch {
    // A disabled or unavailable graphics API must not prevent a report.
  } finally {
    try {
      gl?.getExtension('WEBGL_lose_context')?.loseContext()
    } catch {
      // Some engines disallow explicitly losing a context.
    }
  }
}

export async function probeWebviewCapabilities(): Promise<PageCapabilities> {
  const page: PageCapabilities = {}
  try {
    page.userAgent = navigator.userAgent
  } catch {
    // The probe also works in contexts without navigator.
  }
  try {
    page.devicePixelRatio = window.devicePixelRatio
  } catch {
    // A window is optional.
  }
  page.webgl = probeWebGl()

  await Promise.all([
    (async () => {
      try {
        const data = await (
          navigator as CapabilityNavigator
        ).userAgentData?.getHighEntropyValues([
          'fullVersionList',
          'platformVersion',
        ])
        page.fullVersionList = data?.fullVersionList
        page.platformVersion = data?.platformVersion
      } catch {
        // User agent hints may be unavailable or denied.
      }
    })(),
    (async () => {
      try {
        const gpu = (navigator as CapabilityNavigator).gpu
        page.webgpu = { available: gpu != null }
        const adapter = await gpu?.requestAdapter()
        if (adapter?.info) {
          const { vendor, architecture, device, description } = adapter.info
          page.webgpu.adapter = { vendor, architecture, device, description }
        }
      } catch {
        // Adapter details can be unavailable even when WebGPU is present.
      }
    })(),
  ])

  return page
}
