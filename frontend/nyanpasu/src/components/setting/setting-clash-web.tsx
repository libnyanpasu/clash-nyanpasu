import { useLockFn } from 'ahooks'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import AddIcon from '@mui/icons-material/Add'
import {
  Box,
  Chip,
  Divider,
  IconButton,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material'
import Grid from '@mui/material/Grid'
import { useClashInfo, useSetting } from '@nyanpasu/interface'
import { BaseCard, BaseDialog, Expand } from '@nyanpasu/ui'
import { ClashWebItem, extractServer, openWebUrl, renderChip } from './modules'

const AddRecordButton = ({ onClick }: { onClick: () => void }) => {
  const { t } = useTranslation()

  return (
    <Tooltip title={t('New Item')}>
      <IconButton onClick={onClick}>
        <AddIcon />
      </IconButton>
    </Tooltip>
  )
}

export const SettingClashWeb = () => {
  const { t } = useTranslation()

  const { value, upsert } = useSetting('web_ui_list')

  const { data } = useClashInfo()

  const labels = useMemo(() => {
    const { host, port } = extractServer(data?.server)

    return {
      host,
      port,
      secret: data?.secret,
    }
  }, [data])

  const [open, setOpen] = useState(false)

  const [editString, setEditString] = useState('')

  const [editIndex, setEditIndex] = useState<number | null>(null)

  const deleteItem = useLockFn(async (index: number) => {
    await upsert(
      value ? value.slice(0, index).concat(value.slice(index + 1)) : null,
    )
  })

  const updateItem = useLockFn(async () => {
    const list = [...(value || [])]

    if (!list) return

    if (editIndex !== null) {
      list[editIndex] = editString
    } else {
      list.push(editString)
    }

    await upsert(list)
  })

  return (
    <>
      <BaseCard
        label={t('Web UI')}
        labelChildren={
          <AddRecordButton
            onClick={() => {
              setEditString('')
              setEditIndex(null)
              setOpen(true)
            }}
          />
        }
      >
        {value && (
          <Grid container sx={{ mt: 1 }} spacing={2}>
            {value.map((item, index) => {
              return (
                <Grid
                  key={index}
                  size={{
                    xs: 12,
                    xl: 6,
                  }}
                >
                  <ClashWebItem
                    label={renderChip(item, labels)}
                    onOpen={() => openWebUrl(item, labels)}
                    onEdit={() => {
                      setEditIndex(index)
                      setEditString(item)
                      setOpen(true)
                    }}
                    onDelete={() => deleteItem(index)}
                  />
                </Grid>
              )
            })}
          </Grid>
        )}
      </BaseCard>

      <BaseDialog
        title={editIndex != null ? t('Edit Item') : t('New Item')}
        open={open}
        onClose={() => {
          setOpen(false)
          setEditIndex(null)
        }}
        onOk={() => {
          updateItem()
          setOpen(false)
          setEditIndex(null)
          setEditString('')
        }}
        ok={t('Ok')}
        close={t('Close')}
        contentStyle={{ overflow: editString ? 'auto' : 'hidden' }}
        divider
      >
        <Box display="flex" flexDirection="column" gap={1}>
          <Typography variant="h5">{t('Input')}</Typography>

          <TextField
            sx={{ width: '100%' }}
            value={editString}
            variant="outlined"
            multiline
            placeholder={t(`Support %host %port, and %secret`)}
            onChange={(e) => setEditString(e.target.value)}
          />

          <Typography sx={{ userSelect: 'text' }}>
            {t('Replace host, port, and secret with')}
          </Typography>

          <Box display="flex" gap={1}>
            {Object.entries(labels).map(([key], index) => {
              return <Chip key={index} size="small" label={`%${key}`} />
            })}
          </Box>

          <Expand open={Boolean(editString) || false}>
            <Box display="flex" flexDirection="column" gap={1}>
              <Divider sx={{ mt: 1, mb: 1 }} />

              <Typography variant="h5">{t('Result')}</Typography>

              <Typography sx={{ userSelect: 'text' }} component="div">
                {renderChip(editString, labels)}
              </Typography>
            </Box>
          </Expand>
        </Box>
      </BaseDialog>
    </>
  )
}

export default SettingClashWeb
