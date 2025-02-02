import { createElement } from 'react'
import { useTranslation } from 'react-i18next'
import { languageQuirks } from '@/utils/language'
import { SvgIconComponent } from '@mui/icons-material'
import { alpha, ListItemButton, ListItemIcon, useTheme } from '@mui/material'
import { useNyanpasu } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'
import { useMatch, useNavigate } from '@tanstack/react-router'

export const RouteListItem = ({
  name,
  path,
  icon,
  onlyIcon,
}: {
  name: string
  path: string
  icon: SvgIconComponent
  onlyIcon?: boolean
}) => {
  const { t } = useTranslation()

  const { palette } = useTheme()
  const match = useMatch({
    strict: false,
    shouldThrow: false,
    from: path as never,
  })

  const navigate = useNavigate()

  const { nyanpasuConfig } = useNyanpasu()

  return (
    <ListItemButton
      className={cn(
        onlyIcon ? '!mx-auto !size-16 !rounded-3xl' : '!rounded-full !pr-14',
      )}
      sx={[
        match
          ? {
              backgroundColor: alpha(palette.primary.main, 0.3),
            }
          : {
              backgroundColor: alpha(palette.background.paper, 0.15),
            },
        match
          ? {
              '&:hover': {
                backgroundColor: alpha(palette.primary.main, 0.5),
              },
            }
          : {
              '&:hover': {
                backgroundColor: null,
              },
            },
      ]}
      onClick={() =>
        navigate({
          to: path,
        })
      }
    >
      <ListItemIcon>
        {createElement(icon, {
          sx: {
            fill: match ? palette.primary.main : undefined,
          },
          className: onlyIcon ? '!size-8' : undefined,
        })}
      </ListItemIcon>
      {!onlyIcon && (
        <div
          className={cn(
            'w-full pt-1 pb-1 text-nowrap',
            nyanpasuConfig?.language &&
              languageQuirks[nyanpasuConfig?.language].drawer.itemClassNames,
          )}
          style={{ color: match ? palette.primary.main : undefined }}
        >
          {t(`label_${name}`)}
        </div>
      )}
    </ListItemButton>
  )
}

export default RouteListItem
