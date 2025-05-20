import { useTranslation } from 'react-i18next'
import { FilledInputProps, TextField } from '@mui/material'
import { alpha } from '@nyanpasu/ui'
import { useLogContext } from './log-provider'

export const LogFilter = () => {
  const { t } = useTranslation()

  const { filterText, setFilterText } = useLogContext()

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),

      fieldset: {
        border: 'none',
      },
    }),
  }

  return (
    <TextField
      hiddenLabel
      autoComplete="off"
      spellCheck="false"
      value={filterText}
      placeholder={t('Filter conditions')}
      onChange={(e) => setFilterText(e.target.value)}
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      slotProps={{
        input: inputProps,
      }}
    />
  )
}
