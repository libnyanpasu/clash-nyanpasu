import { Reorder } from 'framer-motion'
import { memo, PointerEvent, useRef, useState, useTransition } from 'react'
import { useTranslation } from 'react-i18next'
import { Menu as MenuIcon } from '@mui/icons-material'
import { Button, ListItemButton, Menu, MenuItem } from '@mui/material'
import { ProfileQueryResultItem } from '@nyanpasu/interface'
import { alpha, cleanDeepClickEvent } from '@nyanpasu/ui'

const longPressDelay = 200

interface Context {
  global: boolean
  scoped: boolean
}

export const ChainItem = memo(function ChainItem({
  item,
  selected,
  context,
  onClick,
  onChainEdit,
}: {
  item: ProfileQueryResultItem
  selected?: boolean
  context?: Context
  onClick: () => Promise<void>
  onChainEdit: () => void
}) {
  const { t } = useTranslation()

  const [isPending, startTransition] = useTransition()

  const handleClick = () => {
    startTransition(onClick)
  }

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null)

  const menuMapping = {
    Apply: () => handleClick(),
    'Edit Info': () => onChainEdit(),
    'Open File': () => item.view && item.view(),
    Delete: () => item.drop && item.drop(),
  }

  const handleMenuClick = (func: () => void) => {
    setAnchorEl(null)
    func()
  }

  // const controls = useDragControls();

  const onLongPress = (e: PointerEvent) => {
    cleanDeepClickEvent(e)
    // controls.start(e);
  }

  const longPressTimerRef = useRef<number | null>(null)

  return (
    <>
      <Reorder.Item
        css={{
          zIndex: 100,
        }}
        value={item.uid}
        // dragListener={false}
        // dragControls={controls}
        onPointerDown={(e: PointerEvent) => {
          longPressTimerRef.current = window.setTimeout(() => {
            longPressTimerRef.current = null
            onLongPress(e as unknown as PointerEvent)
          }, longPressDelay)
        }}
        onPointerUp={(e: PointerEvent) => {
          if (longPressTimerRef.current) {
            clearTimeout(longPressTimerRef.current!)
          } else {
            cleanDeepClickEvent(e)
            longPressTimerRef.current = null
          }
        }}
      >
        <ListItemButton
          className="!mt-2 !mb-2 !flex !justify-between gap-2"
          sx={[
            {
              borderRadius: 4,
            },
            (theme) => ({
              backgroundColor: selected
                ? alpha(theme.vars.palette.primary.main, 0.3)
                : alpha(theme.vars.palette.secondary.main, 0.1),
            }),
            (theme) => ({
              '&:hover': {
                backgroundColor: selected
                  ? alpha(theme.vars.palette.primary.main, 0.5)
                  : null,
              },
            }),
          ]}
          onClick={handleClick}
          disabled={isPending}
        >
          <div className="truncate py-1">
            <span>{item.name}</span>
            <div className="mt-1 flex gap-1">
              {context?.global && (
                <span className="rounded bg-blue-500 px-1 py-0.5 text-xs text-white">
                  G
                </span>
              )}
              {context?.scoped && (
                <span className="rounded bg-green-500 px-1 py-0.5 text-xs text-white">
                  S
                </span>
              )}
            </div>
          </div>

          <Button
            size="small"
            color="primary"
            className="!size-8 !min-w-0"
            loading={isPending}
            onClick={(e) => {
              cleanDeepClickEvent(e)
              setAnchorEl(e!.currentTarget as HTMLButtonElement)
            }}
          >
            <MenuIcon />
          </Button>
        </ListItemButton>
      </Reorder.Item>
      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(menuMapping).map(([key, func], index) => {
          return (
            <MenuItem
              key={index}
              onClick={(e) => {
                cleanDeepClickEvent(e)
                handleMenuClick(func)
              }}
            >
              {t(key)}
            </MenuItem>
          )
        })}
      </Menu>
    </>
  )
})

export default ChainItem
