import { filesize } from 'filesize'
import { useEffect, useRef, useState } from 'react'
import { Download, Upload } from '@mui/icons-material'
import { Paper } from '@mui/material'
import { useClashConnections } from '@nyanpasu/interface'
import { darken, lighten } from '@nyanpasu/ui'

export default function ConnectionTotal() {
  const {
    query: { data: clashConnections },
  } = useClashConnections()

  const latestClashConnections = clashConnections?.at(-1)

  const [downloadHighlight, setDownloadHighlight] = useState(false)
  const [uploadHighlight, setUploadHighlight] = useState(false)

  const downloadHighlightTimerRef = useRef<number | null>(null)
  const uploadHighlightTimerRef = useRef<number | null>(null)

  useEffect(() => {
    if (
      latestClashConnections?.downloadTotal &&
      latestClashConnections?.downloadTotal > 0
    ) {
      setDownloadHighlight(true)
      if (downloadHighlightTimerRef.current) {
        clearTimeout(downloadHighlightTimerRef.current)
      }
      downloadHighlightTimerRef.current = window.setTimeout(() => {
        setDownloadHighlight(false)
      }, 300)
    }
  }, [latestClashConnections?.downloadTotal])

  useEffect(() => {
    if (
      latestClashConnections?.uploadTotal &&
      latestClashConnections?.uploadTotal > 0
    ) {
      setUploadHighlight(true)
      if (uploadHighlightTimerRef.current) {
        clearTimeout(uploadHighlightTimerRef.current)
      }
      uploadHighlightTimerRef.current = window.setTimeout(() => {
        setUploadHighlight(false)
      }, 300)
    }
  }, [latestClashConnections?.uploadTotal])

  if (!latestClashConnections) {
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
                theme.vars.palette.primary.main,
                downloadHighlight ? 0.9 : 0.3,
              ),
              ...theme.applyStyles('dark', {
                color: lighten(
                  theme.vars.palette.primary.main,
                  downloadHighlight ? 0.2 : 0.9,
                ),
              }),
            }),
          ]}
        />{' '}
        <span className="font-mono text-xs">
          {filesize(latestClashConnections.downloadTotal, { pad: true })}
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
                theme.vars.palette.primary.main,
                uploadHighlight ? 0.9 : 0.3,
              ),
              ...theme.applyStyles('dark', {
                color: lighten(
                  theme.vars.palette.primary.main,
                  downloadHighlight ? 0.2 : 0.9,
                ),
              }),
            }),
          ]}
        />{' '}
        <span className="font-mono text-xs">
          {filesize(latestClashConnections.uploadTotal, { pad: true })}
        </span>
      </Paper>
    </div>
  )
}
