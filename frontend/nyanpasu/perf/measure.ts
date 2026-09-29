import type { ProfilerOnRenderCallback } from 'react'

export type FrameSample = {
  // Longest gap between two animation frames; 16.7 ms at 60 Hz is smooth.
  maxFrameGap: number
  framesOver50: number
  // Longest task the browser reported (Chromium only; 0 elsewhere).
  longTaskMax: number
  // From the action to the first DOM mutation it caused: the synchronous
  // JavaScript (router + React render and commit) that blocks the frame.
  syncTime: number
  // React render time summed over every commit in the window.
  reactRender: number
  // Every frame gap, for eyeballing where the drops are.
  gaps: number[]
}

let reactRender = 0

// Pass to a <Profiler> around the tree under test.
export const onProfilerRender: ProfilerOnRenderCallback = (
  _id,
  _phase,
  actualDuration,
) => {
  reactRender += actualDuration
}

// Runs `action` at the start of a frame and records frame timing for `ms`.
export async function measureFrames(
  action: () => void,
  ms: number,
): Promise<FrameSample> {
  const longTasks: number[] = []
  const longTaskObserver = new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      longTasks.push(entry.duration)
    }
  })

  try {
    longTaskObserver.observe({ type: 'longtask', buffered: false })
  } catch {
    // WebKit and Firefox do not report long tasks.
  }

  let actionAt = 0
  let mutatedAt = 0
  const mutationObserver = new MutationObserver(() => {
    mutatedAt ||= performance.now()
  })
  mutationObserver.observe(document.body, { childList: true, subtree: true })

  reactRender = 0
  const gaps: number[] = []
  let last = 0
  let running = true
  const loop = (time: number) => {
    gaps.push(time - last)
    last = time

    if (running) {
      requestAnimationFrame(loop)
    }
  }

  requestAnimationFrame((time) => {
    last = time
    requestAnimationFrame(loop)
    actionAt = performance.now()
    action()
  })

  await new Promise((resolve) => setTimeout(resolve, ms))
  running = false
  longTaskObserver.disconnect()
  mutationObserver.disconnect()

  return {
    maxFrameGap: Math.max(...gaps),
    framesOver50: gaps.filter((gap) => gap > 50).length,
    longTaskMax: Math.max(0, ...longTasks),
    syncTime: mutatedAt ? mutatedAt - actionAt : 0,
    reactRender,
    gaps,
  }
}

export function summarize(samples: FrameSample[]) {
  const average = (key: Exclude<keyof FrameSample, 'gaps'>) =>
    Number(
      (samples.reduce((sum, s) => sum + s[key], 0) / samples.length).toFixed(1),
    )

  return {
    avgMaxFrameGap: average('maxFrameGap'),
    avgFramesOver50: average('framesOver50'),
    avgLongTaskMax: average('longTaskMax'),
    avgSyncTime: average('syncTime'),
    avgReactRender: average('reactRender'),
  }
}

// Logs every synchronous layout read that takes 2 ms or more, with the call
// site. A slow read means the DOM was dirty, so the read forced a style
// recalc or layout in the middle of the task.
export function traceForcedReflows() {
  const wrap = (target: object, name: string, getter: boolean) => {
    const descriptor = Object.getOwnPropertyDescriptor(target, name)!
    const original = (getter ? descriptor.get : descriptor.value) as (
      ...args: unknown[]
    ) => unknown
    const timed = function (this: unknown, ...args: unknown[]) {
      const start = performance.now()
      const result = original.apply(this, args)
      const duration = performance.now() - start

      if (duration >= 2) {
        const stack = (new Error().stack ?? '')
          .split('\n')
          .slice(2, 6)
          .map((line) => line.trim().split('/').pop())
          .join(' < ')
        console.log(`REFLOW ${name} ${duration.toFixed(1)}ms ${stack}`)
      }

      return result
    }

    Object.defineProperty(
      target,
      name,
      getter ? { ...descriptor, get: timed } : { ...descriptor, value: timed },
    )
  }

  wrap(Element.prototype, 'getBoundingClientRect', false)
  wrap(HTMLElement.prototype, 'offsetWidth', true)
  wrap(HTMLElement.prototype, 'offsetHeight', true)
  wrap(Element.prototype, 'scrollWidth', true)
  wrap(Element.prototype, 'scrollHeight', true)
  wrap(window, 'getComputedStyle', false)
}
