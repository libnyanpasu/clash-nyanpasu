import { AnimatePresence, type Transition, type Variant } from 'framer-motion'
import { useSetting } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { useMatch, useMatches } from '@tanstack/react-router'
import { AnimatedOutlet } from '../router/animated-outlet'

type PageVariantKey = 'initial' | 'visible' | 'hidden'

type PageVariant = {
  [key in PageVariantKey]: Variant
}

const commonTransition = {
  type: 'spring',
  bounce: 0,
  duration: 0.35,
} satisfies Transition

export const pageTransitionVariants: { [name: string]: PageVariant } = {
  blur: {
    initial: { opacity: 0, filter: 'blur(10px)' },
    visible: { opacity: 1, filter: 'blur(0px)' },
    hidden: { opacity: 0, filter: 'blur(10px)' },
  },
  slide: {
    initial: {
      translateY: '30%',
      opacity: 0,
      scale: 0.95,
    },
    visible: {
      translateY: '0%',
      opacity: 1,
      scale: 1,
      transition: commonTransition,
    },
    hidden: {
      opacity: 0,
      scale: 0.9,
      transition: commonTransition,
    },
  },
  transparent: {
    initial: { opacity: 0 },
    visible: { opacity: 1 },
    hidden: { opacity: 0 },
  },
}

export default function PageTransition({ className }: { className?: string }) {
  const { value: lightenAnimationEffects } = useSetting(
    'lighten_animation_effects',
  )

  const matches = useMatches()
  const match = useMatch({ strict: false })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  const variants = lightenAnimationEffects
    ? pageTransitionVariants.transparent
    : pageTransitionVariants.slide

  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <AnimatedOutlet
        className={cn('page-transition', className)}
        key={nextMatch ? nextMatch.id : ''}
        layout
        layoutId={nextMatch.id}
        variants={variants}
        initial="initial"
        animate="visible"
        exit="hidden"
      />
    </AnimatePresence>
  )
}
