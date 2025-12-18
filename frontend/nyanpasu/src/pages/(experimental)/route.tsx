import { AnimatedOutletPreset } from '@/components/router/animated-outlet'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import Header from './_modules/header'
import Navbar from './_modules/navbar'

export const Route = createFileRoute('/(experimental)')({
  component: RouteComponent,
})

const AppContent = () => {
  return (
    <AnimatedOutletPreset
      className={cn(
        'h-[calc(100vh-40px-64px)]',
        'sm:h-[calc(100vh-40px-48px)]',
        'overflow-hidden',
        'bg-white dark:bg-black',
      )}
      data-slot="app-content"
    />
  )
}

function RouteComponent() {
  return (
    <div
      className={cn(
        'flex max-h-dvh min-h-dvh flex-col',
        'bg-white dark:bg-black',
      )}
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
