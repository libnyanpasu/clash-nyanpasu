import { filesize } from 'filesize'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Download, Upload } from '@mui/icons-material'
import { darken, lighten, Paper } from '@mui/material'
import { Connection, useClashWS } from '@nyanpasu/interface'

export default function ConnectionTotal() {
  const {
    connections: { latestMessage },
  } = useClashWS()
  const [downloadHighlight, setDownloadHighlight] = useState(false)
  const [uploadHighlight, setUploadHighlight] = useState(false)

  const downloadHighlightTimerRef = useRef<number | null>(null)
  const uploadHighlightTimerRef = useRef<number | null>(null)

  const result = useMemo(() => {
    if (!latestMessage?.data) return null
    return JSON.parse(latestMessage.data) as Connection.Response
  }, [latestMessage])

  useEffect(() => {
    if (result?.downloadTotal && result?.downloadTotal > 0) {
      setDownloadHighlight(true)
      if (downloadHighlightTimerRef.current) {
        clearTimeout(downloadHighlightTimerRef.current)
      }
      downloadHighlightTimerRef.current = window.setTimeout(() => {
        setDownloadHighlight(false)
      }, 300)
    }
  }, [result?.downloadTotal])

  useEffect(() => {
    if (result?.uploadTotal && result?.uploadTotal > 0) {
      setUploadHighlight(true)
      if (uploadHighlightTimerRef.current) {
        clearTimeout(uploadHighlightTimerRef.current)
      }
      uploadHighlightTimerRef.current = window.setTimeout(() => {
        setUploadHighlight(false)
      }, 300)
    }
  }, [result?.uploadTotal])

  if (!result) {
    return null
  }

  return (
    <div className="flex gap-2">
      <Paper
        elevation={0}
        className="flex min-h-8 items-center justify-center gap-1 px-2"
        sx={{
          borderRadius: '1em',
        }}
      >
        <Download
          className="scale-75"
          sx={[
            (theme) => ({
              color: darken(
                theme.palette.primary.main,
                downloadHighlight ? 0.9 : 0.3,
              ),
              ...theme.applyStyles('dark', {
                color: lighten(
                  theme.palette.primary.main,
                  downloadHighlight ? 0.2 : 0.9,
                ),
              }),
            }),
          ]}
        />{' '}
        <span className="font-mono text-xs">
          {filesize(result.downloadTotal, { pad: true })}
        </span>
      </Paper>
      <Paper
        elevation={0}
        className="flex min-h-8 items-center justify-center gap-1 px-2"
        sx={{
          borderRadius: '1em',
        }}
      >
        <Upload
          className="scale-75"
          sx={[
            (theme) => ({
              color: darken(
                theme.palette.primary.main,
                uploadHighlight ? 0.9 : 0.3,
              ),
              ...theme.applyStyles('dark', {
                color: lighten(
                  theme.palette.primary.main,
                  downloadHighlight ? 0.2 : 0.9,
                ),
              }),
            }),
          ]}
        />{' '}
        <span className="font-mono text-xs">
          {filesize(result.uploadTotal, { pad: true })}
        </span>
      </Paper>
    </div>
  )
}
