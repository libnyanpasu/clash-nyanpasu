import { useTranslation } from 'react-i18next'
import { FilledInputProps, TextField, TextFieldProps } from '@mui/material'
import { alpha } from '@nyanpasu/ui'

export const HeaderSearch = (props: TextFieldProps) => {
  const { t } = useTranslation()

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),

      '&::before': {
        display: 'none',
      },

      '&::after': {
        display: 'none',
      },
    }),
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
      slotProps={{
        input: inputProps,
      }}
      {...props}
    />
  )
}

export default HeaderSearch
