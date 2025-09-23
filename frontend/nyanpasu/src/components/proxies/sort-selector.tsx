import { useAtom } from 'jotai'
import { memo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { proxyGroupSortAtom } from '@/store'
import { Menu, MenuItem } from '@mui/material'
import { MUIButton as Button } from '@nyanpasu/ui'

export const SortSelector = memo(function SortSelector() {
  const { t } = useTranslation()

  const [proxyGroupSort, setProxyGroupSort] = useAtom(proxyGroupSortAtom)

  type SortType = typeof proxyGroupSort

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null)

  const handleClick = (sort: SortType) => {
    setAnchorEl(null)
    setProxyGroupSort(sort)
  }

  const tmaps: { [key: string]: string } = {
    default: 'Sort by default',
    delay: 'Sort by latency',
    name: 'Sort by name',
  }

  return (
    <>
      <Button
        size="small"
        className="!px-2 normal-case"
        style={{
          backgroundColor:
            'color-mix(in oklab, var(--md3-color-primary) 10%, transparent)',
        }}
        onClick={(e) => setAnchorEl(e.currentTarget)}
      >
        {t(tmaps[proxyGroupSort])}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(tmaps).map(([key, value], index) => {
          return (
            <MenuItem key={index} onClick={() => handleClick(key as SortType)}>
              {t(value)}
            </MenuItem>
          )
        })}
      </Menu>
    </>
  )
})

export default SortSelector
