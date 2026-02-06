import { motion, useAnimationControls } from 'framer-motion'
import { useCallback, useEffect, useRef, useState } from 'react'
import { sleep } from '@/utils'
import { cn } from '@nyanpasu/ui'

export default function TextMarquee({
  children,
  className,
  speed = 30,
  gap = 32,
  pauseDuration = 1,
  // pauseOnHover = true,
}: {
  children: React.ReactNode
  className?: string
  speed?: number
  gap?: number
  pauseDuration?: number
  // pauseOnHover?: boolean
}) {
  const containerRef = useRef<HTMLDivElement>(null)

  const textRef = useRef<HTMLDivElement>(null)

  const [shouldAnimate, setShouldAnimate] = useState(false)

  const [textWidth, setTextWidth] = useState(0)

  const controls = useAnimationControls()

  const isHoveredRef = useRef(false)

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
    checkOverflow()

    const resizeObserver = new ResizeObserver(() => {
      checkOverflow()
    })

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => {
      resizeObserver.disconnect()
    }
  }, [checkOverflow, children])

  // Animate when shouldAnimate changes
  useEffect(() => {
    if (!shouldAnimate) {
      controls.set({ x: 0 })
      return
    }

    const totalDistance = textWidth + gap
    const animDuration = totalDistance / speed

    const cancelledRef = { current: false }

    const runAnimationLoop = async () => {
      // Wait at start position
      await sleep(pauseDuration * 1000)

      if (cancelledRef.current) {
        return
      }

      // Check if hovered, wait and retry
      if (isHoveredRef.current) {
        await sleep(100)

        if (!cancelledRef.current) {
          runAnimationLoop()
        }

        return
      }

      // Animate to end
      await controls.start({
        x: -totalDistance,
        transition: {
          duration: animDuration,
          ease: 'linear',
        },
      })

      if (cancelledRef.current) {
        return
      }

      // Reset to start position instantly and loop
      controls.set({ x: 0 })

      if (!cancelledRef.current) {
        runAnimationLoop()
      }
    }

    runAnimationLoop()

    return () => {
      cancelledRef.current = true
      controls.stop()
    }
  }, [shouldAnimate, textWidth, gap, speed, pauseDuration, controls])

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
      className={cn('overflow-hidden', className)}
      data-slot="text-marquee"
    >
      {shouldAnimate ? (
        <motion.div
          className="flex whitespace-nowrap"
          animate={controls}
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
        </motion.div>
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
