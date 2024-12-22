import { AnimatePresence, motion, useIsPresent, Variant } from 'framer-motion'
import { cloneDeep } from 'lodash-es'
import { useContext, useRef } from 'react'
import { useNyanpasu } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import {
  getRouterContext,
  Outlet,
  useMatch,
  useMatches,
} from '@tanstack/react-router'

type PageVariantKey = 'initial' | 'visible' | 'hidden'

type PageVariant = {
  [key in PageVariantKey]: Variant
}

const commonTransition = {
  type: 'spring',
  bounce: 0,
  duration: 0.35,
}

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

function AnimatedOutlet({
  ref,
  className,
  ...others
}: Parameters<typeof motion.div>['0']) {
  const isPresent = useIsPresent()

  const matches = useMatches()
  const prevMatches = useRef(matches)

  const RouterContext = getRouterContext()
  const routerContext = useContext(RouterContext)

  let renderedContext = routerContext

  if (isPresent) {
    prevMatches.current = cloneDeep(matches)
  } else {
    renderedContext = cloneDeep(routerContext)
    renderedContext.__store.state.matches = [
      ...matches.map((m, i) => ({
        ...(prevMatches.current[i] || m),
        id: m.id,
      })),
      ...prevMatches.current.slice(matches.length),
    ]
  }

  return (
    <motion.div ref={ref} className={className} {...others}>
      <RouterContext.Provider value={renderedContext}>
        <Outlet />
      </RouterContext.Provider>
    </motion.div>
  )
}

export default function PageTransition({ className }: { className?: string }) {
  const { nyanpasuConfig } = useNyanpasu()

  const matches = useMatches()
  const match = useMatch({ strict: false })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  const variants = nyanpasuConfig?.lighten_animation_effects
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
