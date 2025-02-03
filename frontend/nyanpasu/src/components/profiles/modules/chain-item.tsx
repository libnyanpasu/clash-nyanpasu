import { useLongPress } from 'ahooks'
import { Reorder, useDragControls } from 'framer-motion'
import {
  memo,
  PointerEvent,
  useCallback,
  useRef,
  useState,
  useTransition,
} from 'react'
import { useTranslation } from 'react-i18next'
import { Menu as MenuIcon } from '@mui/icons-material'
import { LoadingButton } from '@mui/lab'
import { alpha, ListItemButton, Menu, MenuItem, useTheme } from '@mui/material'
import { ProfileQueryResultItem } from '@nyanpasu/interface'
import { cleanDeepClickEvent } from '@nyanpasu/ui'

const longPressDelay = 200

export const ChainItem = memo(function ChainItem({
  item,
  selected,
  onClick,
  onChainEdit,
}: {
  item: ProfileQueryResultItem
  selected?: boolean
  onClick: () => Promise<void>
  onChainEdit: () => void
}) {
  const { t } = useTranslation()

  const { palette } = useTheme()

  // const { deleteProfile, viewProfile } = useClash()

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
            selected
              ? {
                  backgroundColor: alpha(palette.primary.main, 0.3),
                }
              : {
                  backgroundColor: alpha(palette.secondary.main, 0.1),
                },
            selected
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
          onClick={handleClick}
          disabled={isPending}
        >
          <div className="truncate py-1">
            <span>{item.name}</span>
          </div>

          <LoadingButton
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
          </LoadingButton>
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
