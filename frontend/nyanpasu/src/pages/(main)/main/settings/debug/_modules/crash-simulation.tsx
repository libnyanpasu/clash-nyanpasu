import { Button } from '@nyanpasu/ui/button'
import { rpc } from '@/services/rpc'
import { useLockFn } from '@nyanpasu/hooks'
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../../_modules/settings-card'

export default function CrashSimulation() {
  const handleMainThread = useLockFn(async () => {
    await rpc.simulateCrash(true)
  })

  const handleBackgroundThread = useLockFn(async () => {
    await rpc.simulateCrash(false)
  })

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Crash Simulation</SettingsCardHeader>

        <SettingsCardContent>
          <p className="text-sm">
            Panics the backend, which closes the app and opens the crash
            reporter. The main thread and a background thread take different
            exit paths in the panic hook.
          </p>

          <div className="flex flex-wrap items-center gap-2">
            <Button variant="flat" onClick={handleMainThread}>
              Panic on main thread
            </Button>

            <Button variant="flat" onClick={handleBackgroundThread}>
              Panic on background thread
            </Button>
          </div>
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  )
}
