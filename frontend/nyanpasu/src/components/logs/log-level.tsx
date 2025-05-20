import { useState } from 'react'
import { Button, Menu, MenuItem } from '@mui/material'
import { alpha } from '@nyanpasu/ui'
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
        sx={(theme) => ({
          textTransform: 'none',
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
        })}
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
