import { Button } from '@/components/ui/button'
import { setEnabledExperimentalRouter } from '@/utils/experimental'
import { useNavigate } from '@tanstack/react-router'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

export default function ExperimentalSwitch() {
  const navigate = useNavigate()
  const handleClick = () => {
    setEnabledExperimentalRouter(false)
    navigate({ to: '/' })
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
