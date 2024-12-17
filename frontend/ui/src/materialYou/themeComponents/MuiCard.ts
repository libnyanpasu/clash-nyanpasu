import { Theme } from '@mui/material'
import { Components } from '@mui/material/styles/components'

export const MuiCard: Components<Theme>['MuiCard'] = {
  defaultProps: {
    sx: {
      borderRadius: 6,
      elevation: 0,
    },
  },
}
