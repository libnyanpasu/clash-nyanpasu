import { useSetAtom } from 'jotai'
import { Button } from '@/components/ui/button'
import { memorizedRoutePathAtom } from '@/store'
import { setEnabledExperimentalRouter } from '@/utils/experimental'
import { useNavigate } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ExperimentalSwitch() {
  const navigate = useNavigate()

  const setMemorizedNavigate = useSetAtom(memorizedRoutePathAtom)

  const handleClick = () => {
    setEnabledExperimentalRouter(false)
    navigate({ to: '/dashboard' })
    setMemorizedNavigate('/dashboard')
  }

  return (
    <SettingsCard data-slot="experimental-switch-card">
      <SettingsCardContent
        className="flex items-center justify-between px-3"
        data-slot="experimental-switch-card-content"
      >
        <div>Switch to Legacy UI</div>

        <Button variant="flat" onClick={handleClick}>
          Im sure, continue!
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  )
}
