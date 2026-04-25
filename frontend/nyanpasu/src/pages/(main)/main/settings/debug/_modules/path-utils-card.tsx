import { Button, ButtonProps } from '@/components/ui/button'
import TextMarquee from '@/components/ui/text-marquee'
import { useLockFn } from '@/hooks/use-lock-fn'
import { m } from '@/paraglide/messages'
import { commands } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/utils'

const PathButton = ({
  className,
  children,
  ...props
}: Omit<ButtonProps, 'variant'>) => {
  return (
    <Button
      variant="raised"
      className={cn(
        'h-18 w-full rounded-3xl px-5 text-left font-bold',
        className,
      )}
      {...props}
    >
      <TextMarquee>{children}</TextMarquee>
    </Button>
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
  )
}
