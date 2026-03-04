import { AnimatePresence, motion, useIsPresent, Variants } from 'framer-motion'
import { ComponentProps, useRef } from 'react'
import {
  Outlet,
  RouterContextProvider,
  useMatch,
  useMatches,
  useRouter,
  useRouterState,
} from '@tanstack/react-router'

type TransitionDirection = 1 | -1

const directionalSlideVariants = {
  forward: {
    initial: {
      translateX: '30%',
      opacity: 0,
    },
    visible: {
      translateX: '0%',
      opacity: 1,
    },
    hidden: {
      translateX: '-30%',
      opacity: 0,
    },
  },
  backward: {
    initial: {
      translateX: '-30%',
      opacity: 0,
    },
    visible: {
      translateX: '0%',
      opacity: 1,
    },
    hidden: {
      translateX: '30%',
      opacity: 0,
    },
  },
} satisfies Record<'forward' | 'backward', Variants>

function getDirectionalVariant(direction: TransitionDirection) {
  return direction === 1
    ? directionalSlideVariants.forward
    : directionalSlideVariants.backward
}

export function AnimatedOutlet({
  ref,
  ...props
}: ComponentProps<typeof motion.div>) {
  const isPresent = useIsPresent()

  const matches = useMatches()
  const prevMatches = useRef(matches)

  const router = useRouter()

  // Frozen router for the exit animation, created once when isPresent becomes false
  const frozenRouterRef = useRef<typeof router | null>(null)

  let renderedRouter = router

  if (isPresent) {
    prevMatches.current = matches
    frozenRouterRef.current = null
  } else {
    if (!frozenRouterRef.current) {
      // Build patched matches: old route data (prevMatches) but new match IDs
      const patched = [
        ...matches.map((m, i) => ({
          ...(prevMatches.current[i] || m),
          id: m.id,
        })),
        ...prevMatches.current.slice(matches.length),
      ]

      // Snapshot of router state with old route's matches
      const patchedState = { ...router.__store.state, matches: patched }

      // Create a fake store that always returns the frozen patched state.
      // Object.create delegates everything else (subscribe, atom, etc.) to the real
      // store via the prototype chain, so subscriptions still work — but the snapshot
      // always returns patchedState, which never changes, so there are no re-renders.
      const fakeStore = Object.create(router.__store)
      Object.defineProperty(fakeStore, 'get', {
        value: () => patchedState,
        configurable: true,
      })
      Object.defineProperty(fakeStore, 'state', {
        get: () => patchedState,
        configurable: true,
      })

      // Create a fake router that delegates everything to the real router except __store
      const fakeRouter = Object.create(router)
      Object.defineProperty(fakeRouter, '__store', {
        value: fakeStore,
        configurable: true,
      })

      frozenRouterRef.current = fakeRouter
    }

    // force type safety
    renderedRouter = frozenRouterRef.current!
  }

  return (
    <motion.div ref={ref} {...props}>
      <RouterContextProvider router={renderedRouter}>
        <Outlet />
      </RouterContextProvider>
    </motion.div>
  )
}

export function AnimatedOutletPreset(props: ComponentProps<typeof motion.div>) {
  const matches = useMatches()
  const match = useMatch({ strict: false })
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  const id = nextMatch ? nextMatch.id : ''
  const prevPathRef = useRef(pathname)
  const directionRef = useRef<TransitionDirection>(1)

  if (prevPathRef.current !== pathname) {
    const prevPath = prevPathRef.current
    const nextPath = pathname

    if (nextPath.startsWith(`${prevPath}/`)) {
      directionRef.current = 1
    } else if (prevPath.startsWith(`${nextPath}/`)) {
      directionRef.current = -1
    } else {
      // Non-ancestor navigation (including sibling routes) uses forward animation.
      directionRef.current = 1
    }

    prevPathRef.current = pathname
  }

  const direction = directionRef.current
  const selectedVariants = getDirectionalVariant(direction)

  return (
    <AnimatePresence mode="popLayout" initial={false} custom={direction}>
      <AnimatedOutlet
        key={id}
        custom={direction}
        layout="position"
        initial="initial"
        animate="visible"
        exit="hidden"
        variants={{
          initial: (customDirection: TransitionDirection) =>
            getDirectionalVariant(customDirection).initial,
          visible: selectedVariants.visible,
          hidden: (customDirection: TransitionDirection) =>
            getDirectionalVariant(customDirection).hidden,
        }}
        transition={{
          type: 'spring',
          bounce: 0.1,
          duration: 0.35,
        }}
        {...props}
      />
    </AnimatePresence>
  )
}
