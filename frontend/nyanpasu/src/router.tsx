import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useState } from 'react'
import { onSplashDone } from '@/utils/splash-signal'
import { cn } from '@nyanpasu/ui'
import { Metaballs } from '@paper-design/shaders-react'
import {
  createRouter,
  RouterProvider as RouterProviderPrimitive,
} from '@tanstack/react-router'
import AnimatedLogo from './components/logo/animated-logo'
import { routeTree } from './route-tree.gen'

// Set up a Router instance
const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
})

// Register things for typesafety
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

const SplashOverlay = () => {
  const [show, setShow] = useState(true)

  useEffect(() => {
    return onSplashDone(() => setShow(false))
  }, [])

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          className={cn(
            'bg-mixed-background fixed inset-0 z-50 flex h-dvh w-full items-center justify-center',
          )}
          initial={{
            opacity: 0,
          }}
          animate={{
            opacity: 1,
          }}
          exit={{
            opacity: 0,
            scale: 1.15,
            transition: {
              duration: 0.4,
            },
          }}
        >
          <Metaballs
            className="size-full"
            colors={['#6e33cc', '#ff5500', '#ffc105', '#ffc800', '#f585ff']}
            colorBack="#00000000"
            count={10}
            size={0.8}
            scale={2.8}
            speed={1}
          />

          <div
            className="absolute inset-0 grid place-content-center"
            data-tauri-drag-region
          >
            <AnimatedLogo className="size-40" indeterminate />
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

export const RouterProvider = () => {
  return (
    <>
      <RouterProviderPrimitive router={router} />

      <SplashOverlay />
    </>
  )
}
