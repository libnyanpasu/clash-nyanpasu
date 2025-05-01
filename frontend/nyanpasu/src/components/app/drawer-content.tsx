import getSystem from '@/utils/get-system'
import { getRoutesWithIcon } from '@/utils/routes-utils'
import { Box } from '@mui/material'
import { cn } from '@nyanpasu/ui'
import AnimatedLogo from '../layout/animated-logo'
import RouteListItem from './modules/route-list-item'

export const DrawerContent = ({
  className,
  onlyIcon,
}: {
  className?: string
  onlyIcon?: boolean
}) => {
  const routes = getRoutesWithIcon()

  return (
    <Box
      className={cn(
        'p-4',
        getSystem() === 'macos' ? 'pt-14' : 'pt-8',
        'w-full',
        'h-full',
        'flex',
        'flex-col',
        'gap-4',
        className,
      )}
      sx={[
        {
          backgroundColor: 'var(--background-color-alpha)',
        },
      ]}
      data-tauri-drag-region
    >
      <div className="mx-2 flex items-center justify-center gap-4">
        <div className="h-full max-h-28 max-w-28" data-tauri-drag-region>
          <AnimatedLogo className="h-full w-full" data-tauri-drag-region />
        </div>

        {!onlyIcon && (
          <div
            className="mt-1 flex-1 text-lg font-bold whitespace-pre-wrap"
            data-tauri-drag-region
          >
            {'Clash\nNyanpasu'}
          </div>
        )}
      </div>

      <div className="scrollbar-hidden flex flex-col gap-2 !overflow-x-hidden overflow-y-auto">
        {Object.entries(routes).map(([name, { path, icon }]) => {
          return (
            <RouteListItem
              key={name}
              name={name}
              path={path}
              icon={icon}
              onlyIcon={onlyIcon}
            />
          )
        })}
      </div>
    </Box>
  )
}

export default DrawerContent
