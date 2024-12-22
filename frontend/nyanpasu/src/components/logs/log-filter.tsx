import { useTranslation } from 'react-i18next'
import { alpha, FilledInputProps, TextField, useTheme } from '@mui/material'

export interface LogFilterProps {
  value: string
  onChange: (value: string) => void
}

export const LogFilter = ({ value, onChange }: LogFilterProps) => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      fieldset: {
        border: 'none',
      },
    },
  }

  return (
    <TextField
      hiddenLabel
      autoComplete="off"
      spellCheck="false"
      value={value}
      placeholder={t('Filter conditions')}
      onChange={(e) => onChange(e.target.value)}
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      InputProps={inputProps}
    />
  )
}
