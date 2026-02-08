import { motion, useAnimationControls } from 'framer-motion'
import {
  ComponentProps,
  KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
} from 'react'
import LogoSvg from '@/assets/image/logo.svg?react'

const FAST_SPRING = [0.22, 1, 0.36, 1] as const // fast attack, soft landing
const DRAMATIC_PRESENT = [0.2, 0.8, 0.2, 1] as const // dramatic entrance
const GENTLE_SYMMETRIC_S_CURVE = [0.45, 0.05, 0.55, 0.95] as const // gentle symmetric S-curve

export default function AnimatedLogo({
  indeterminate,
  ...props
}: Omit<ComponentProps<typeof motion.div>, 'children'> & {
  indeterminate?: boolean
}) {
  const logoControls = useAnimationControls()
  const intensityRef = useRef(0)
  const directionRef = useRef(1)
  const resetTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const indeterminateRef = useRef(false)

  useEffect(() => {
    return () => {
      if (resetTimeoutRef.current) {
        clearTimeout(resetTimeoutRef.current)
      }
    }
  }, [])

  const startSway = useCallback(() => {
    logoControls.start({
      rotate: [0, 2, 0, -2, 0],
      scale: [1, 1.02, 1, 0.98, 1],
      transition: {
        duration: 2.4,
        ease: GENTLE_SYMMETRIC_S_CURVE,
        repeat: Infinity,
        repeatType: 'loop',
      },
    })
  }, [logoControls])

  // Indeterminate mode: init animation then continuous slow sway
  useEffect(() => {
    indeterminateRef.current = !!indeterminate

    if (indeterminate) {
      let cancelled = false

      const run = async () => {
        // Init animation: blur fade in
        logoControls.set({
          filter: 'blur(10px)',
          opacity: 0,
          scale: 2,
        })
        await logoControls.start({
          filter: 'blur(0px)',
          opacity: 1,
          scale: 1,
          transition: {
            duration: 1,
            ease: DRAMATIC_PRESENT,
          },
        })

        if (cancelled) {
          return
        }

        // Continuous slow sway
        startSway()
      }

      run()

      return () => {
        cancelled = true
      }
    } else {
      // Reset to idle
      logoControls.stop()
      logoControls.start({
        rotate: 0,
        scale: 1,
        filter: 'blur(0px)',
        opacity: 1,
        transition: { duration: 0.4, ease: FAST_SPRING },
      })
    }
  }, [indeterminate, logoControls, startSway])

  const scheduleReset = useCallback(() => {
    if (resetTimeoutRef.current) {
      clearTimeout(resetTimeoutRef.current)
    }

    // reset intensity after 1 seconds
    resetTimeoutRef.current = setTimeout(async () => {
      intensityRef.current = 0

      if (indeterminateRef.current) {
        // Smoothly reset scale first, then resume sway
        await logoControls.start({
          scale: 1,
          rotate: 0,
          transition: {
            duration: 0.4,
            ease: FAST_SPRING,
          },
        })
        if (indeterminateRef.current) {
          startSway()
        }
      } else {
        logoControls.start({
          scale: 1,
          transition: {
            duration: 0.4,
            ease: FAST_SPRING,
          },
        })
      }
    }, 1000)
  }, [logoControls, startSway])

  const triggerShake = useCallback(() => {
    const nextIntensity = Math.min(12, intensityRef.current + 1)
    intensityRef.current = nextIntensity

    // alternate direction each click
    const d = directionRef.current
    directionRef.current = -d

    // non-linear amplitude: ramps up slowly then accelerates
    const amp = 4 + Math.pow(nextIntensity, 1.4) * 0.8

    // calculate the scale based on the intensity
    const scale = 1 + Math.min(0.75, Math.pow(nextIntensity, 1.3) * 0.03)

    // apply the animation to logo
    logoControls.start({
      rotate: [-amp * d, amp * d, -amp * 0.8 * d, amp * 0.8 * d, 0],
      scale,
      transition: {
        duration: 0.5,
        ease: 'linear',
        rotate: {
          duration: 0.5,
          ease: [0.1, 0.8, 0.2, 1],
        },
      },
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
