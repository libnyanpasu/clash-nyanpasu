import { Button, ButtonProps } from '@/components/ui/button'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { SettingsCard, SettingsCardContent } from '../../_modules/settings-card'

const PathButton = ({ className, ...props }: Omit<ButtonProps, 'variant'>) => {
  return (
    <Button
      variant="raised"
      className={cn(
        'h-14 w-full rounded-3xl px-5 text-left font-bold',
        className,
      )}
      {...props}
    />
  )
}

export default function PathUtilsCard() {
  const handleOpenConfigDirectory = useLockFn(async () => {
    await commands.openAppConfigDir()
  })

  const handleOpenDataDirectory = useLockFn(async () => {
    await commands.openAppDataDir()
  })

  const handleOpenCoreDirectory = useLockFn(async () => {
    await commands.openCoreDir()
  })

  const handleOpenLogDirectory = useLockFn(async () => {
    await commands.openLogsDir()
  })

  return (
    <SettingsCard data-slot="path-utils-card">
      <SettingsCardContent
        data-slot="path-utils-card-content"
        className="flex flex-col gap-3 px-2"
      >
        <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
          <PathButton onClick={handleOpenConfigDirectory}>
            {m.settings_debug_utils_open_config_directory()}
          </PathButton>

          <PathButton onClick={handleOpenDataDirectory}>
            {m.settings_debug_utils_open_data_directory()}
          </PathButton>

          <PathButton onClick={handleOpenCoreDirectory}>
            {m.settings_debug_utils_open_core_directory()}
          </PathButton>

          <PathButton onClick={handleOpenLogDirectory}>
            {m.settings_debug_utils_open_log_directory()}
          </PathButton>
        </div>
      </SettingsCardContent>
    </SettingsCard>
  )
}
