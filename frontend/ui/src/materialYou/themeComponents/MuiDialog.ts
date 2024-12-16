import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiDialog: Components<Theme>['MuiDialog'] = {
  styleOverrides: {
    paper: {
      borderRadius: 24,
    },
  },
}
