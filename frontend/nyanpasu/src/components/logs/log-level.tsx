import { useState } from 'react'
import { alpha, Button, Menu, MenuItem, useTheme } from '@mui/material'
import { useLogContext } from './log-provider'

export const LogLevel = () => {
  const { logLevel, setLogLevel } = useLogContext()

  const { palette } = useTheme()

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
        sx={{
          textTransform: 'none',
          backgroundColor: alpha(palette.primary.main, 0.1),
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
