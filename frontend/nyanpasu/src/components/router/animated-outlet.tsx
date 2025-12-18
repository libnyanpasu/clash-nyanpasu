import { AnimatePresence, motion, useIsPresent } from 'framer-motion'
import { cloneDeep } from 'lodash-es'
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
