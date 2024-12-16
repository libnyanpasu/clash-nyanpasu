import { useTranslation } from 'react-i18next'
import {
  alpha,
  FilledInputProps,
  TextField,
  TextFieldProps,
  useTheme,
} from '@mui/material'

export const HeaderSearch = (props: TextFieldProps) => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      '&::before': {
        display: 'none',
      },

      '&::after': {
        display: 'none',
      },
    },
  }

  return (
    <TextField
      autoComplete="off"
      spellCheck="false"
      hiddenLabel
      placeholder={t('Filter conditions')}
      variant="filled"
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      InputProps={inputProps}
      {...props}
    />
  )
}

export default HeaderSearch
