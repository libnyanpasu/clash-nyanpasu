import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { cn } from '@nyanpasu/ui'
import { createFileRoute } from '@tanstack/react-router'
import ProfileQuickImport from './_modules/profile-quick-import'
import { useProfilesContext } from './_modules/profiles-provider'

export const Route = createFileRoute('/(experimental)/experimental/profiles/')({
  component: RouteComponent,
})

const SidebarToggleButton = () => {
  const { sidebarOpen, setSidebarOpen } = useProfilesContext()

  const handleClick = () => {
    setSidebarOpen(!sidebarOpen)
  }

  return (
    <Button icon onClick={handleClick}>
      <ChevronRightRounded
        className={cn(
          'size-6 transition-transform duration-300',
          sidebarOpen && 'rotate-180',
        )}
      />
    </Button>
  )
}

function RouteComponent() {
  return (
    <>
      <div className="absolute top-0 left-0 flex items-center justify-between gap-2 p-4 pb-0">
        <SidebarToggleButton />
      </div>

      <div
        className={cn(
          'flex flex-col items-center justify-center gap-8',
          'mx-auto w-4/5 sm:w-3/5',
          'h-[calc(100vh-40px-64px)]',
          'sm:h-[calc(100vh-40px-48px)]',
        )}
      >
        <p className="text-center text-2xl font-bold">Next Step</p>

        <div className="w-full">
          <ProfileQuickImport />
        </div>

        <div className={cn('grid w-full grid-cols-1 gap-4', 'md:grid-cols-3')}>
          <Card>
            <CardContent>
              <span className="font-bold">Current Profile:</span>
              <span className="font-bold">Default</span>
            </CardContent>
          </Card>

          <Card>
            <CardContent>
              <span className="font-bold">Import Profile</span>

              <div className="flex gap-2">
                <Button>Local</Button>
                <Button>Remote</Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardContent>
              <span className="font-bold">Profile Status</span>

              <span>Global Chians: 1</span>
              <span>Scoped Chians: 2</span>
            </CardContent>
          </Card>
        </div>
      </div>
    </>
  )
}
