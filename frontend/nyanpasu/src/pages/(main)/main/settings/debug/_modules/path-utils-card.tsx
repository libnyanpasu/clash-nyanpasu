import { Button, ButtonProps } from '@nyanpasu/ui/button'
import TextMarquee from '@nyanpasu/ui/text-marquee'
import { m } from '@/paraglide/messages'
import { rpc } from '@/services/rpc'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { useLockFn } from '@nyanpasu/hooks'
import { unwrapResult } from '@nyanpasu/rpc'
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
    await rpc.openAppConfigDir()
  })

  const handleOpenDataDirectory = useLockFn(async () => {
    await rpc.openAppDataDir()
  })

  const handleOpenCoreDirectory = useLockFn(async () => {
    await rpc.openCoreDir()
  })

  const handleOpenLogDirectory = useLockFn(async () => {
    await rpc.openLogsDir()
  })

  const handleOpenBackupDirectory = useLockFn(async () => {
    await rpc.openBackupsDir()
  })

  const handleCreateBackup = useLockFn(async () => {
    try {
      const backup = unwrapResult(await rpc.createConfigBackup())

      await message(
        m.settings_debug_utils_create_backup_success({ name: backup.name }),
        {
          kind: 'info',
        },
      )
    } catch (error) {
      await message(
        `${m.settings_debug_utils_create_backup_error()}: ${formatError(error)}`,
        {
          kind: 'error',
          error,
        },
      )
    }
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

      <PathButton onClick={handleOpenBackupDirectory}>
        {m.settings_debug_utils_open_backup_directory()}
      </PathButton>

      <PathButton onClick={handleCreateBackup}>
        {m.settings_debug_utils_create_backup()}
      </PathButton>
    </div>
  )
}
