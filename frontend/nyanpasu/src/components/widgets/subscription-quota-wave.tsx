import { area, curveBasis, line, range } from 'd3'
import { useReducedMotion } from 'motion/react'
import { useEffect, useRef } from 'react'

// Two identical periods allow a seamless compositor-driven horizontal loop.
const samples = range(-20, 2021, 10)

export function SubscriptionQuotaWave({
  percent,
  waveStyle,
  animateWave,
}: {
  percent: number
  waveStyle: 'single' | 'double'
  animateWave: boolean
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const reducedMotion = useReducedMotion()
  const level = 200 * (1 - Math.max(0, Math.min(100, percent)) / 100)
  const amplitude = animateWave ? Math.min(8, level, 200 - level) : 0

  useEffect(() => {
    const root = rootRef.current
    if (
      !root ||
      !animateWave ||
      reducedMotion ||
      percent <= 0 ||
      percent >= 100
    )
      return

    const animations = Array.from(root.querySelectorAll('svg')).map(
      (wave, index) =>
        wave.animate(
          [{ transform: 'translateX(0)' }, { transform: 'translateX(-50%)' }],
          { duration: index === 0 ? 9000 : 13000, iterations: Infinity },
        ),
    )
    let visible = true
    const sync = () => {
      for (const animation of animations) {
        if (visible && !document.hidden) animation.play()
        else animation.pause()
      }
    }
    const observer = new IntersectionObserver((entries) => {
      visible = entries.at(-1)?.isIntersecting ?? false
      sync()
    })
    observer.observe(root)
    document.addEventListener('visibilitychange', sync)
    sync()

    return () => {
      observer.disconnect()
      document.removeEventListener('visibilitychange', sync)
      animations.forEach((animation) => animation.cancel())
    }
  }, [reducedMotion, percent, waveStyle, animateWave])

  return (
    <div
      ref={rootRef}
      aria-hidden="true"
      data-slot="subscription-quota-wave"
      data-percent={percent}
      className="pointer-events-none absolute inset-0 overflow-hidden"
    >
      <div
        className="size-full transition-transform duration-700 ease-in-out motion-reduce:transition-none"
        style={{ transform: `translateY(${level / 2}%)` }}
      >
        {(!animateWave || waveStyle === 'single' ? [0] : [0, 1]).map(
          (layer) => {
            const y = (x: number) =>
              amplitude * Math.sin((x / 1000) * Math.PI * 2 + layer * Math.PI)
            const outline = line<number>()
              .x((x) => x)
              .y(y)
              .curve(curveBasis)
            const fill = area<number>()
              .x((x) => x)
              .y0(200)
              .y1(y)
              .curve(curveBasis)

            return (
              <svg
                key={layer}
                viewBox="0 0 2000 200"
                preserveAspectRatio="none"
                className="absolute inset-y-0 left-0 h-full w-[200%] overflow-visible"
              >
                {layer === 0 && (
                  <path d={fill(samples) ?? ''} className="fill-primary/10" />
                )}
                <path
                  d={outline(samples) ?? ''}
                  fill="none"
                  className={
                    layer === 0 ? 'stroke-primary' : 'stroke-primary/40'
                  }
                  strokeWidth={2}
                  vectorEffect="non-scaling-stroke"
                />
              </svg>
            )
          },
        )}
      </div>
    </div>
  )
}
