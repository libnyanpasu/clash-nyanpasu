import {
  AnimatePresence,
  motion,
  PresenceContext,
  useIsPresent,
  Variants,
} from 'motion/react'
import { ComponentProps, use, useMemo, useRef, type ReactNode } from 'react'
import {
  ClashConnectionDetailsFreezeBoundary,
  ClashWSFreezeBoundary,
} from '@nyanpasu/query'
import {
  Outlet,
  RouterContextProvider,
  useMatch,
  useMatches,
  useRouter,
  useRouterState,
} from '@tanstack/react-router'
import { historyDirection } from './cross-navigation'

type TransitionDirection = 1 | -1

// A whole `transform` value (not `translateX`) lets Motion run the slide as a
// browser animation off the main thread; independent transforms are driven
// from the JS frame loop, which stalls while the incoming page mounts. The
// settled page drops the transform so it is not the containing block of
// `position: fixed` descendants.
const directionalSlideVariants = {
  forward: {
    initial: {
      transform: 'translateX(30%)',
      opacity: 0,
    },
    visible: {
      transform: 'translateX(0%)',
      opacity: 1,
      transitionEnd: { transform: 'none' },
    },
    hidden: {
      transform: 'translateX(-30%)',
      opacity: 0,
    },
  },
  backward: {
    initial: {
      transform: 'translateX(-30%)',
      opacity: 0,
    },
    visible: {
      transform: 'translateX(0%)',
      opacity: 1,
      transitionEnd: { transform: 'none' },
    },
    hidden: {
      transform: 'translateX(30%)',
      opacity: 0,
    },
  },
} satisfies Record<'forward' | 'backward', Variants>

function getDirectionalVariant(direction: TransitionDirection) {
  return direction === 1
    ? directionalSlideVariants.forward
    : directionalSlideVariants.backward
}

export type AnimatedOutletProps = ComponentProps<typeof motion.div> & {
  /**
   * Wraps the routed page, for example in a provider of the parent layout's
   * context. A leaving page keeps the wrapper it had when it started leaving,
   * so a value given here does not change, nor re-render it, during the slide.
   */
  renderOutlet?: (outlet: ReactNode) => ReactNode
}

export function AnimatedOutlet({
  ref,
  renderOutlet,
  ...props
}: AnimatedOutletProps) {
  const isPresent = useIsPresent()

  // AnimatePresence gives a leaving child a new presence with each of its
  // renders. The page is told once that it leaves, or every AnimatePresence
  // and motion component in it renders again with each render of the layout
  // during the slide.
  const presence = use(PresenceContext)
  const pagePresence = useMemo(() => presence, [presence?.isPresent])

  const matches = useMatches()
  const prevMatches = useRef(matches)

  const router = useRouter()
  const prevState = useRef(router.state)

  // Frozen router for the exit animation, created once when isPresent becomes false
  const frozenRouterRef = useRef<typeof router | null>(null)

  let renderedRouter = router

  if (isPresent) {
    prevMatches.current = matches
    prevState.current = router.state
    frozenRouterRef.current = null
  } else {
    if (!frozenRouterRef.current) {
      // The leaving page's matches and the router state it last rendered.
      // The router sets the next location as it starts loading and commits
      // the matches after, so the location they were resolved for is the
      // page's.
      const frozenMatches = prevMatches.current
      const { resolvedLocation, location } = prevState.current
      const frozenState = {
        ...prevState.current,
        location: resolvedLocation ?? location,
        matches: frozenMatches,
      }

      // Helper: create a static frozen store for the exit animation.
      // useSelector (@tanstack/react-store) needs .get() and .subscribe(); a
      // no-op unsubscribe means the frozen store never triggers re-renders.
      // Use function syntax to avoid <T> being parsed as JSX in .tsx files.
      function frozenStore<T>(value: T) {
        return {
          state: value,
          get: () => value,
          subscribe: () => ({ unsubscribe: () => {} }),
        }
      }

      // The router keys its match stores by route id: <Outlet> renders the
      // route after its own in `ids`, and <Match> and useMatch read
      // `getMatchStore(routeId)`. Each store the leaving page can read is
      // replaced with its value from when the page started leaving.
      const byRoute = new Map<string, ReturnType<typeof frozenStore>>(
        frozenMatches.map((match) => [match.routeId, frozenStore(match)]),
      )
      const value = (store: unknown) => ({ value: store, configurable: true })
      const fakeStores = Object.create(router.stores, {
        ids: value(frozenStore(frozenMatches.map((match) => match.routeId))),
        matches: value(frozenStore(frozenMatches)),
        byRoute: value(byRoute),
        getMatchStore: value(
          (routeId: string) => byRoute.get(routeId) ?? frozenStore(undefined),
        ),
        status: value(frozenStore(frozenState.status)),
        location: value(frozenStore(frozenState.location)),
        resolvedLocation: value(frozenStore(frozenState.resolvedLocation)),
        __store: value(frozenStore(frozenState)),
      })

      // A router that delegates everything to the real one but its stores and state
      const fakeRouter = Object.create(router, {
        stores: value(fakeStores),
        state: { get: () => frozenState, configurable: true },
      })

      frozenRouterRef.current = fakeRouter
    }

    // force type safety
    renderedRouter = frozenRouterRef.current!
  }

  return (
    <motion.div ref={ref} {...props}>
      <RouterContextProvider router={renderedRouter}>
        <PresenceContext value={pagePresence}>
          {/* The exiting page keeps the stream samples it had when it started
              leaving, so live updates do not re-render it during the slide. */}
          <ClashWSFreezeBoundary frozen={!isPresent}>
            <ClashConnectionDetailsFreezeBoundary frozen={!isPresent}>
              {renderOutlet ? renderOutlet(<Outlet />) : <Outlet />}
            </ClashConnectionDetailsFreezeBoundary>
          </ClashWSFreezeBoundary>
        </PresenceContext>
      </RouterContextProvider>
    </motion.div>
  )
}

/**
 * Renders child routes with the shared, history-aware slide transition.
 *
 * Layout contract: the animated page must fill the available content height,
 * including when the route contains little content. Give the parent a resolved
 * height or viewport-derived minimum height and a column flex layout; make this
 * outlet grow with `flex-1`. Preserve that height chain through any scroll-area
 * content wrappers. Clip the surrounding scroll surface so sliding pages cannot
 * overflow into adjacent navigation. Content must still be able to grow and
 * scroll; `h-full` alone is insufficient when an ancestor has an auto height.
 *
 * @example
 * ```tsx
 * <div className="flex min-h-full flex-col overflow-clip">
 *   <AnimatedOutletPreset className="flex flex-1 flex-col" />
 * </div>
 * ```
 */
export function AnimatedOutletPreset(props: AnimatedOutletProps) {
  const matches = useMatches()
  const match = useMatch({ strict: false })
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const historyIndex = useRouterState({
    select: (state) => state.location.state.__TSR_index,
  })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  const id = nextMatch ? nextMatch.id : ''
  const prevPathRef = useRef(pathname)
  const prevHistoryIndexRef = useRef(historyIndex)
  const directionRef = useRef<TransitionDirection>(1)

  if (prevPathRef.current !== pathname) {
    const prevPath = prevPathRef.current
    const nextPath = pathname

    if (historyDirection(prevHistoryIndexRef.current, historyIndex) === -1) {
      // A step back through history slides back, whatever the paths are.
      directionRef.current = -1
    } else if (nextPath.startsWith(`${prevPath}/`)) {
      directionRef.current = 1
    } else if (prevPath.startsWith(`${nextPath}/`)) {
      directionRef.current = -1
    } else {
      // Non-ancestor navigation (including sibling routes) uses forward animation.
      directionRef.current = 1
    }

    prevPathRef.current = pathname
  }

  prevHistoryIndexRef.current = historyIndex

  const direction = directionRef.current
  const selectedVariants = getDirectionalVariant(direction)

  return (
    <AnimatePresence mode="popLayout" initial={false} custom={direction}>
      <AnimatedOutlet
        key={id}
        custom={direction}
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
