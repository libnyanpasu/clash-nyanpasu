import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ClearRounded, ContentCopyRounded, Download } from '@mui/icons-material'
import {
  alpha,
  CircularProgress,
  FilledInputProps,
  IconButton,
  TextField,
  Tooltip,
  useTheme,
} from '@mui/material'
import { useProfile } from '@nyanpasu/interface'
import { readText } from '@tauri-apps/plugin-clipboard-manager'

export const QuickImport = () => {
  const { t } = useTranslation()

  const { palette } = useTheme()

  const [url, setUrl] = useState('')

  const [loading, setLoading] = useState(false)

  const { create } = useProfile()

  const onCopyLink = async () => {
    const text = await readText()

    if (text) {
      setUrl(text)
    }
  }

  const endAdornment = () => {
    if (loading) {
      return <CircularProgress size={20} />
    }

    if (url) {
      return (
        <>
          <Tooltip title={t('Clear')}>
            <IconButton size="small" onClick={() => setUrl('')}>
              <ClearRounded fontSize="inherit" />
            </IconButton>
          </Tooltip>

          <Tooltip title={t('Download')}>
            <IconButton size="small" onClick={handleImport}>
              <Download fontSize="inherit" />
            </IconButton>
          </Tooltip>
        </>
      )
    }

    return (
      <Tooltip title={t('Paste')}>
        <IconButton size="small" onClick={onCopyLink}>
          <ContentCopyRounded fontSize="inherit" />
        </IconButton>
      </Tooltip>
    )
  }

  const handleImport = async () => {
    try {
      setLoading(true)

      await create.mutateAsync({
        type: 'url',
        data: {
          url,
          option: null,
        },
      })
    } finally {
      setUrl('')
      setLoading(false)
    }
  }

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      fieldset: {
        border: 'none',
      },
    },
    endAdornment: endAdornment(),
  }

  return (
    <TextField
      hiddenLabel
      fullWidth
      autoComplete="off"
      spellCheck="false"
      value={url}
      placeholder={t('Profile URL')}
      onChange={(e) => setUrl(e.target.value)}
      onKeyDown={(e) => url !== '' && e.key === 'Enter' && handleImport()}
      sx={{ input: { py: 1, px: 2 } }}
      InputProps={inputProps}
    />
  )
}
