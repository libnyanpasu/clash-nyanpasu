import {
  PauseCircleOutlineRounded,
  PlayCircleOutlineRounded,
} from '@mui/icons-material'
import { IconButton } from '@mui/material'
import { useClashLogs } from '@nyanpasu/interface'

export const LogToggle = () => {
  const { status, disable, enable } = useClashLogs()

  const handleClick = () => {
    if (status) {
      disable()
    } else {
      enable()
    }
  }

  return (
    <IconButton size="small" color="inherit" onClick={handleClick}>
      {status ? <PauseCircleOutlineRounded /> : <PlayCircleOutlineRounded />}
    </IconButton>
  )
}

export default LogToggle
