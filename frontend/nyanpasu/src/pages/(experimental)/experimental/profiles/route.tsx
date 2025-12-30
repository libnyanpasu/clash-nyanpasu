import { motion, Transition } from 'framer-motion'
import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { AppContentScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import {
  ProfilesProvider,
  useProfilesContext,
} from './_modules/profiles-provider'
import ProfilesSidebar from './_modules/profiles-sidebar'

export const Route = createFileRoute('/(experimental)/experimental/profiles')({
  component: RouteComponent,
})

const sidebarTransition = {
  type: 'spring',
  stiffness: 400,
  damping: 40,
} satisfies Transition

function InnerRouteComponent() {
  const { sidebarOpen } = useProfilesContext()

  return (
    <motion.div
      className="flex"
      data-slot="profiles-container"
      transition={sidebarTransition}
    >
      <motion.div
        className="overflow-hidden"
        initial={false}
        animate={{
          width: sidebarOpen ? 256 : 0,
          opacity: sidebarOpen ? 1 : 0,
        }}
        transition={sidebarTransition}
      >
        <AppContentScrollArea
          className={cn('z-50 w-64', 'bg-surface-variant/5')}
          data-slot="profiles-sidebar-scroll-area"
        >
          <ProfilesSidebar />
        </AppContentScrollArea>
      </motion.div>

      <motion.div
        className="flex-1 overflow-clip"
        layout
        transition={sidebarTransition}
      >
        <AppContentScrollArea
          className={cn('group/profiles-content', 'overflow-clip')}
          data-slot="profiles-content-scroll-area"
        >
          <div
            className={cn(
              'container mx-auto w-full max-w-7xl',
              'min-h-[calc(100vh-40px-64px)]',
              'sm:min-h-[calc(100vh-40px-48px)]',
            )}
            data-slot="profiles-content"
          >
            <AnimatedOutletPreset />
          </div>
        </AppContentScrollArea>
      </motion.div>
    </motion.div>
  )
}

function RouteComponent() {
  return (
    <ProfilesProvider>
      <InnerRouteComponent />
    </ProfilesProvider>
  )
}
