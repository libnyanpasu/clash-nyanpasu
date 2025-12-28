import ChevronRightRounded from '~icons/material-symbols/chevron-right-rounded'
import { Button } from '@/components/ui/button'
import { cn } from '@nyanpasu/ui'
import ProfileQuickImport from '../../_modules/profile-quick-import'
import { useProfilesContext } from '../../_modules/profiles-provider'

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

export default function ProfilesHeader() {
  return (
    <div className="flex items-center justify-between gap-2 p-4 pb-0">
      <SidebarToggleButton />

      <ProfileQuickImport />
    </div>
  )
}
