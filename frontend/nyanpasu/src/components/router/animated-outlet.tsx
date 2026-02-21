import { AnimatePresence, motion, useIsPresent } from 'framer-motion'
import { ComponentProps, useContext, useRef } from 'react'
import {
  getRouterContext,
  Outlet,
  useMatch,
  useMatches,
} from '@tanstack/react-router'

export function AnimatedOutlet({
  ref,
  ...props
}: ComponentProps<typeof motion.div>) {
  const isPresent = useIsPresent()

  const matches = useMatches()
  const prevMatches = useRef(matches)

  const RouterContext = getRouterContext()
  const routerContext = useContext(RouterContext)

  // Frozen router for the exit animation, created once when isPresent becomes false
  const frozenRouterRef = useRef<typeof routerContext | null>(null)

  let renderedContext = routerContext

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
      const patchedState = { ...routerContext.__store.state, matches: patched }

      // Create a fake store that always returns the frozen patched state.
      // Object.create delegates everything else (subscribe, atom, etc.) to the real
      // store via the prototype chain, so subscriptions still work — but the snapshot
      // always returns patchedState, which never changes, so there are no re-renders.
      const fakeStore = Object.create(routerContext.__store)
      Object.defineProperty(fakeStore, 'get', {
        value: () => patchedState,
        configurable: true,
      })
      Object.defineProperty(fakeStore, 'state', {
        get: () => patchedState,
        configurable: true,
      })

      // Create a fake router that delegates everything to the real router except __store
      const fakeRouter = Object.create(routerContext)
      Object.defineProperty(fakeRouter, '__store', {
        value: fakeStore,
        configurable: true,
      })

      frozenRouterRef.current = fakeRouter
    }

    // force type safety
    renderedContext = frozenRouterRef.current!
  }

  return (
    <motion.div ref={ref} {...props}>
      <RouterContext.Provider value={renderedContext}>
        <Outlet />
      </RouterContext.Provider>
    </motion.div>
  )
}

export function AnimatedOutletPreset(props: ComponentProps<typeof motion.div>) {
  const matches = useMatches()
  const match = useMatch({ strict: false })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  const id = nextMatch ? nextMatch.id : ''

  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <AnimatedOutlet
        key={id}
        layout
        layoutId={id}
        initial="initial"
        animate="visible"
        exit="hidden"
        variants={{
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
