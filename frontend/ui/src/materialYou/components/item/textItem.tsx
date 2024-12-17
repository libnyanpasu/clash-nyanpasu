import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import Done from '@mui/icons-material/Done'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import ListItem from '@mui/material/ListItem'
import TextField from '@mui/material/TextField'
import { Expand } from '../expand'

export interface TextItemProps {
  value: string
  label: string
  onApply: (value: string) => void
  applyLabel?: string
  placeholder?: string
}

export const TextItem = ({
  value,
  label,
  onApply,
  applyLabel,
  placeholder,
}: TextItemProps) => {
  const { t } = useTranslation()

  const [textString, setTextString] = useState(value)

  return (
    <>
      <ListItem sx={{ pl: 0, pr: 0 }}>
        <TextField
          value={textString}
          label={label}
          variant="outlined"
          sx={{ width: '100%' }}
          multiline
          onChange={(e) => setTextString(e.target.value)}
          placeholder={placeholder}
        />
      </ListItem>

      <Expand open={textString !== value}>
        <Box sx={{ pb: 1 }} display="flex" justifyContent="end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => onApply(textString)}
          >
            {applyLabel ?? t('Apply')}
          </Button>
        </Box>
      </Expand>
    </>
  )
}
