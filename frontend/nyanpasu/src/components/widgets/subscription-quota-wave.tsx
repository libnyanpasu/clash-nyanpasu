import { cva } from 'class-variance-authority'
import { area, curveBasis, line, range } from 'd3'
import { useReducedMotion } from 'motion/react'
import { useEffect, useRef } from 'react'
import type { SubscriptionQuotaWaveStyle } from './widget-config'

// Whole periods must fit twice into the 2000-unit width for a seamless loop.
const samples = range(-20, 2021, 10)

const waveFillVariants = cva('', {
  variants: {
    fill: {
      back: 'fill-primary-container/55',
      front: 'fill-primary-container/80',
      tint: 'fill-primary/10',
    },
  },
})

const waveStrokeVariants = cva('', {
  variants: {
    stroke: {
      strong: 'stroke-primary',
      faint: 'stroke-primary/40',
    },
  },
})

type WaveLayer = {
  name: string
  phase: number
  duration: number
  direction: PlaybackDirection
  fill?: 'back' | 'front' | 'tint'
  stroke?: 'strong' | 'faint'
  // A resting waterline is a single line, so the extra layer is dropped.
  animatedOnly?: boolean
}

const waveStyles = {
  // Water body with two translucent layers, like the website's client preview.
  filled: {
    amplitude: 6,
    period: 500,
    layers: [
      {
        name: 'back',
        phase: Math.PI,
        duration: 12000,
        direction: 'normal',
        fill: 'back',
      },
      {
        name: 'front',
        phase: 0,
        duration: 18000,
        direction: 'reverse',
        fill: 'front',
      },
    ],
  },
  // The original wave lines over a faint tint.
  outline: {
    amplitude: 8,
    period: 1000,
    layers: [
      {
        name: 'primary',
        phase: 0,
        duration: 9000,
        direction: 'normal',
        fill: 'tint',
        stroke: 'strong',
      },
      {
        name: 'secondary',
        phase: Math.PI,
        duration: 13000,
        direction: 'normal',
        stroke: 'faint',
        animatedOnly: true,
      },
    ],
  },
} satisfies Record<
  SubscriptionQuotaWaveStyle,
  { amplitude: number; period: number; layers: WaveLayer[] }
>

function visibleLayers(
  waveStyle: SubscriptionQuotaWaveStyle,
  animateWave: boolean,
): WaveLayer[] {
  return waveStyles[waveStyle].layers.filter(
    (layer: WaveLayer) => animateWave || !layer.animatedOnly,
  )
}

export function SubscriptionQuotaWave({
  percent,
  waveStyle,
  animateWave,
}: {
  percent: number
  waveStyle: SubscriptionQuotaWaveStyle
  animateWave: boolean
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const reducedMotion = useReducedMotion()
  const { amplitude: maxAmplitude, period } = waveStyles[waveStyle]
  const level = 200 * (1 - Math.max(0, Math.min(100, percent)) / 100)
  const amplitude = animateWave ? Math.min(maxAmplitude, level, 200 - level) : 0

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

    const layers = visibleLayers(waveStyle, animateWave)
    const animations = Array.from(root.querySelectorAll('svg')).map(
      (wave, index) =>
        wave.animate(
          [{ transform: 'translateX(0)' }, { transform: 'translateX(-50%)' }],
          {
            duration: layers[index].duration,
            direction: layers[index].direction,
            iterations: Infinity,
          },
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
        {visibleLayers(waveStyle, animateWave).map((layer) => {
          const y = (x: number) =>
            amplitude * Math.sin((x / period) * Math.PI * 2 + layer.phase)
          const body = area<number>()
            .x((x) => x)
            .y0(200)
            .y1(y)
            .curve(curveBasis)
          const outline = line<number>()
            .x((x) => x)
            .y(y)
            .curve(curveBasis)

          return (
            <svg
              key={layer.name}
              viewBox="0 0 2000 200"
              preserveAspectRatio="none"
              className="absolute inset-y-0 left-0 h-full w-[200%] overflow-visible"
            >
              {layer.fill && (
                <path
                  d={body(samples) ?? ''}
                  className={waveFillVariants({ fill: layer.fill })}
                />
              )}
              {layer.stroke && (
                <path
                  d={outline(samples) ?? ''}
                  fill="none"
                  className={waveStrokeVariants({ stroke: layer.stroke })}
                  strokeWidth={2}
                  vectorEffect="non-scaling-stroke"
                />
              )}
            </svg>
          )
        })}
      </div>
    </div>
  )
}
