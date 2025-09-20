import { createElement } from 'react'
import { useTranslation } from 'react-i18next'
import { languageQuirks } from '@/utils/language'
import { SvgIconComponent } from '@mui/icons-material'
import { Box, ListItemButton, ListItemIcon, Tooltip } from '@mui/material'
import { useSetting } from '@nyanpasu/interface'
import { alpha, cn } from '@nyanpasu/ui'
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
  const match = useMatch({
    strict: false,
    shouldThrow: false,
    from: path as never,
  })

  const navigate = useNavigate()

  const { value: language } = useSetting('language')

  const listItemButton = (
    <ListItemButton
      className={cn(
        onlyIcon ? '!mx-auto !size-16 !rounded-3xl' : '!rounded-full !pr-14',
      )}
      sx={[
        (theme) => ({
          backgroundColor: match
            ? alpha(theme.vars.palette.primary.main, 0.3)
            : alpha(theme.vars.palette.background.paper, 0.15),
        }),
        (theme) => ({
          '&:hover': {
            backgroundColor: match
              ? alpha(theme.vars.palette.primary.main, 0.5)
              : null,
          },
        }),
      ]}
      onClick={() => {
        navigate({
          to: path,
        })
      }}
    >
      <ListItemIcon>
        {createElement(icon, {
          sx: (theme) => ({
            fill: match ? theme.vars.palette.primary.main : undefined,
          }),
          className: onlyIcon ? '!size-8' : undefined,
        })}
      </ListItemIcon>
      {!onlyIcon && (
        <Box
          className={cn(
            'w-full pt-1 pb-1 text-nowrap',
            language && languageQuirks[language].drawer.itemClassNames,
          )}
          sx={(theme) => ({
            color: match ? theme.vars.palette.primary.main : undefined,
          })}
        >
          {t(`label_${name}`)}
        </Box>
      )}
    </ListItemButton>
  )

  return onlyIcon ? (
    <Tooltip title={t(`label_${name}`)}>{listItemButton}</Tooltip>
  ) : (
    listItemButton
  )
}

export default RouteListItem
