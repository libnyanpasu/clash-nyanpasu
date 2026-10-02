import { useMedia } from 'react-use'

/**
 * The traffic page's transition: the standard easing, and none at all when
 * the system asks for reduced motion.
 */
export function usePageTransition() {
  const reducedMotion = useMedia('(prefers-reduced-motion: reduce)', false)

  return {
    reducedMotion,
    transition: {
      duration: reducedMotion ? 0 : 0.32,
      ease: [0.2, 0, 0, 1] as const,
    },
  }
}
