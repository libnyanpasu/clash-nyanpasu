import { AnimatePresence, motion, useIsPresent } from 'framer-motion'
import { cloneDeep } from 'lodash-es'
import { ComponentProps, useContext, useRef } from 'react'
import { cn } from '@nyanpasu/ui'
import {
  createFileRoute,
  getRouterContext,
  Outlet,
  useMatch,
  useMatches,
} from '@tanstack/react-router'
import Header from './_modules/header'
import Navbar from './_modules/navbar'

export const Route = createFileRoute('/(experimental)')({
  component: RouteComponent,
})

function AnimatedOutlet({ ref, ...props }: ComponentProps<typeof motion.div>) {
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

const AppContent = () => {
  const matches = useMatches()
  const match = useMatch({ strict: false })
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1
  const nextMatch = matches[nextMatchIndex]

  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <AnimatedOutlet
        className={cn(
          'h-[calc(100vh-40px-64px)]',
          'sm:h-[calc(100vh-40px-48px)]',
          'overflow-hidden',
          'bg-white dark:bg-black',
        )}
        data-slot="app-content"
        key={nextMatch.id}
        layout
        layoutId={nextMatch.id}
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
      />
    </AnimatePresence>
  )
}

function RouteComponent() {
  return (
    <div
      className="flex max-h-dvh min-h-dvh flex-col"
      onContextMenu={(e) => {
        e.preventDefault()
      }}
    >
      <Header />

      <div
        className="flex flex-1 flex-col sm:flex-col-reverse"
        data-slot="app-content-container"
      >
        <AppContent />

        <Navbar />
      </div>
    </div>
  )
}
