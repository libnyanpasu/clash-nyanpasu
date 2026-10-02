import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { cn } from '@nyanpasu/utils'

export default function TextMarquee({
  children,
  className,
  speed = 30,
  gap = 32,
  pauseDuration = 1,
  fadeEdges = false,
  fadeWidth = 12,
  // pauseOnHover = true,
}: {
  children: React.ReactNode
  className?: string
  speed?: number
  gap?: number
  pauseDuration?: number
  fadeEdges?: boolean
  fadeWidth?: number
  // pauseOnHover?: boolean
}) {
  const containerRef = useRef<HTMLDivElement>(null)

  const textRef = useRef<HTMLDivElement>(null)

  const [shouldAnimate, setShouldAnimate] = useState(false)

  const [textWidth, setTextWidth] = useState(0)

  const contentRef = useRef<HTMLDivElement>(null)

  // Check if text overflows container
  const checkOverflow = useCallback(() => {
    if (!containerRef.current || !textRef.current) {
      return
    }

    const container = containerRef.current
    const text = textRef.current

    const containerW = container.offsetWidth
    const textW = text.scrollWidth

    setTextWidth(textW)
    setShouldAnimate(textW > containerW)
  }, [])

  // Observe container size changes
  useEffect(() => {
    const resizeObserver = new ResizeObserver(() => {
      checkOverflow()
    })

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => {
      resizeObserver.disconnect()
    }
  }, [checkOverflow])

  // Re-measure when the text changes. `children` is a new element on every
  // parent render, so comparing the rendered text avoids a layout read per
  // render.
  const measuredTextRef = useRef<string | null>(null)

  useEffect(() => {
    const text = textRef.current?.textContent ?? null

    if (text !== measuredTextRef.current) {
      measuredTextRef.current = text
      checkOverflow()
    }
  })

  // Pause at the start, scroll one copy's width, then jump back, forever.
  // A Web Animation on transform runs on the compositor; motion would drive
  // `x` from the JS frame loop.
  useEffect(() => {
    const container = containerRef.current
    const content = contentRef.current

    if (!shouldAnimate || !container || !content) {
      return
    }

    const pause = pauseDuration * 1000
    const totalDistance = textWidth + gap
    const duration = pause + (totalDistance / speed) * 1000

    const animation = content.animate(
      [
        { transform: 'translateX(0)' },
        { transform: 'translateX(0)', offset: pause / duration },
        { transform: `translateX(${-totalDistance}px)` },
      ],
      { duration, iterations: Infinity, easing: 'linear' },
    )

    // Off-screen marquees (scrolled-away list rows) need not keep ticking.
    // Observe the container: the content scrolls out of its own clip, so
    // watching it would pause the animation mid-scroll for good.
    // Several changes can arrive in one callback; the last one is current.
    const visibility = new IntersectionObserver((entries) => {
      if (entries.at(-1)?.isIntersecting) {
        animation.play()
      } else {
        animation.pause()
      }
    })

    visibility.observe(container)

    return () => {
      visibility.disconnect()
      animation.cancel()
    }
  }, [shouldAnimate, textWidth, gap, speed, pauseDuration])

  // const handleMouseEnter = () => {
  //   if (!pauseOnHover) {
  //     return
  //   }

  //   isHoveredRef.current = true
  //   controls.stop()
  // }

  // const handleMouseLeave = () => {
  //   if (!pauseOnHover || !shouldAnimate) {
  //     return
  //   }

  //   isHoveredRef.current = false

  //   resumeAnimation()
  // }

  // const resumeAnimation = () => {
  //   const totalDistance = textWidth + gap

  //   // Resume animation
  //   const marqueeContent = containerRef.current?.querySelector<HTMLDivElement>(
  //     '[data-marquee-content]',
  //   )

  //   if (marqueeContent) {
  //     const transform = window.getComputedStyle(marqueeContent).transform
  //     const matrix = new DOMMatrix(transform)
  //     const currentPosition = matrix.m41

  //     const remainingDistance = -totalDistance - currentPosition
  //     const remainingDuration = Math.abs(remainingDistance) / speed

  //     controls.start({
  //       x: -totalDistance,
  //       transition: {
  //         duration: remainingDuration,
  //         ease: 'linear',
  //       },
  //     })
  //   }
  // }

  return (
    <div
      ref={containerRef}
      className={cn(
        'overflow-hidden',
        fadeEdges && [
          'mask-[linear-gradient(to_right,transparent_0,black_var(--marquee-fade-width),black_calc(100%-var(--marquee-fade-width)),transparent_100%)]',
          '[-webkit-mask-image:linear-gradient(to_right,transparent_0,black_var(--marquee-fade-width),black_calc(100%-var(--marquee-fade-width)),transparent_100%)]',
        ],
        className,
      )}
      data-slot="text-marquee"
      style={
        fadeEdges
          ? ({
              '--marquee-fade-width': `${fadeWidth}px`,
            } as CSSProperties)
          : {}
      }
    >
      {shouldAnimate ? (
        <div
          ref={contentRef}
          className="flex whitespace-nowrap"
          data-slot="text-marquee-content"
        >
          <span
            ref={textRef}
            data-slot="text-marquee-content-item"
            data-index="0"
          >
            {children}
          </span>

          <span
            style={{
              paddingLeft: gap,
            }}
            data-slot="text-marquee-content-item"
            data-index="1"
          >
            {children}
          </span>
        </div>
      ) : (
        <div
          ref={textRef}
          className="truncate"
          data-slot="text-marquee-content"
        >
          {children}
        </div>
      )}
    </div>
  )
}
