import { motion, useAnimationControls } from 'framer-motion'
import {
  ComponentProps,
  KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
} from 'react'
import LogoSvg from '@/assets/image/logo.svg?react'

export default function AnimatedLogo(
  props: Omit<ComponentProps<typeof motion.div>, 'children'>,
) {
  const logoControls = useAnimationControls()
  const intensityRef = useRef(0)
  const resetTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    return () => {
      if (resetTimeoutRef.current) {
        clearTimeout(resetTimeoutRef.current)
      }
    }
  }, [])

  const scheduleReset = useCallback(() => {
    if (resetTimeoutRef.current) {
      clearTimeout(resetTimeoutRef.current)
    }

    // reset intensity after 1 seconds
    resetTimeoutRef.current = setTimeout(() => {
      intensityRef.current = 0
      logoControls.start({
        scale: 1,
        transition: { duration: 0.4, ease: 'easeInOut' },
      })
    }, 1000)
  }, [logoControls])

  const triggerShake = useCallback(() => {
    const nextIntensity = Math.min(12, intensityRef.current + 1)
    intensityRef.current = nextIntensity

    // calculate the scale based on the intensity
    const scale = 1 + Math.min(0.75, Math.pow(nextIntensity, 1.3) * 0.03)

    // apply the animation to logo
    logoControls.start({
      rotate: [-10, 10, -8, 8, 0],
      scale,
      transition: { duration: 0.6, ease: 'easeInOut' },
    })

    scheduleReset()
  }, [logoControls, scheduleReset])

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault()
        triggerShake()
      }
    },
    [triggerShake],
  )

  return (
    <motion.div
      data-slot="app-header-logo"
      data-tauri-drag-region
      role="button"
      tabIndex={0}
      onClick={triggerShake}
      onKeyDown={handleKeyDown}
      animate={logoControls}
      aria-label="Animate logo"
      {...props}
    >
      <LogoSvg
        className="logo-colorized h-full w-full"
        data-tauri-drag-region
      />
    </motion.div>
  )
}
