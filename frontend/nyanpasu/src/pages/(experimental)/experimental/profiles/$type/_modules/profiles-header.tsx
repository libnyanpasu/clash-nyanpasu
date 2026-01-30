import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded'
import { Button } from '@/components/ui/button'
import useIsMobile from '@/hooks/use-is-moblie'
import { Link } from '@tanstack/react-router'
import ProfileQuickImport from '../../_modules/profile-quick-import'

const BackButton = () => {
  return (
    <Button icon className="flex items-center justify-center md:hidden" asChild>
      <Link to="/experimental/profiles">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  )
}

export default function ProfilesHeader() {
  const isMobile = useIsMobile()

  return (
    <div className="flex items-center justify-between gap-2 p-4 pb-0">
      {isMobile && <BackButton />}

      <ProfileQuickImport />
    </div>
  )
}
