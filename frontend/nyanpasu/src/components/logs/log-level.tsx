import { useState } from 'react'
import { Menu, MenuItem } from '@mui/material'
import { MUIButton as Button } from '@nyanpasu/ui'
import { useLogContext } from './log-provider'

export const LogLevel = () => {
  const { logLevel, setLogLevel } = useLogContext()

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null)

  const handleClick = (value: string) => {
    setAnchorEl(null)
    setLogLevel(value)
  }

  const mapping: { [key: string]: string } = {
    all: 'ALL',
    inf: 'INFO',
    warn: 'WARN',
    err: 'ERROR',
  }

  return (
    <>
      <Button
        size="small"
        className="normal-case"
        style={{
          backgroundColor:
            'color-mix(in oklab, var(--md3-color-primary) 10%, transparent)',
        }}
        onClick={(e) => setAnchorEl(e.currentTarget)}
      >
        {mapping[logLevel]}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(mapping).map(([key, value], index) => {
          return (
            <MenuItem key={index} onClick={() => handleClick(key)}>
              {value}
            </MenuItem>
          )
        })}
      </Menu>
    </>
  )
}
