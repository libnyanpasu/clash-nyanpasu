import { version } from '~/package.json'
import { useAsyncEffect } from 'ahooks'
import { type editor } from 'monaco-editor'
import {
  createContext,
  lazy,
  Suspense,
  use,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import {
  Controller,
  SelectElement,
  TextFieldElement,
  useForm,
} from 'react-hook-form-mui'
import { useTranslation } from 'react-i18next'
import { useLatest } from 'react-use'
import { formatError } from '@/utils'
import { message } from '@/utils/notification'
import { Divider, InputAdornment } from '@mui/material'
import { Profile, useClash } from '@nyanpasu/interface'
import { BaseDialog } from '@nyanpasu/ui'
import { LabelSwitch } from '../setting/modules/clash-field'
import { ReadProfile } from './read-profile'

const ProfileMonacoViewer = lazy(() => import('./profile-monaco-viewer'))

export interface ProfileDialogProps {
  profile?: Profile.Item
  open: boolean
  onClose: () => void
}

export type AddProfileContextValue = {
  name: string | null
  desc: string | null
  url: string
}

export const AddProfileContext = createContext<AddProfileContextValue | null>(
  null,
)

export const ProfileDialog = ({
  profile,
  open,
  onClose,
}: ProfileDialogProps) => {
  const { t } = useTranslation()

  const { createProfile, setProfiles, getProfileFile, setProfileFile } =
    useClash()

  const localProfile = useRef('')
  const addProfileCtx = use(AddProfileContext)
  const [localProfileMessage, setLocalProfileMessage] = useState('')

  const { control, watch, handleSubmit, reset, setValue } =
    useForm<Profile.Item>({
      defaultValues: profile || {
        type: 'remote',
        name: addProfileCtx?.name || t(`New Profile`),
        desc: addProfileCtx?.desc || '',
        url: addProfileCtx?.url || '',
        option: {
          // user_agent: "",
          with_proxy: false,
          self_proxy: false,
        },
      },
    })

  useEffect(() => {
    if (addProfileCtx) {
      setValue('url', addProfileCtx.url)
      if (addProfileCtx.desc) setValue('desc', addProfileCtx.desc)
      if (addProfileCtx.name) setValue('name', addProfileCtx.name)
    }
  }, [addProfileCtx, setValue])

  const isRemote = watch('type') === 'remote'

  const [isEdit, setIsEdit] = useState(!!profile)
  useEffect(() => {
    setIsEdit(!!profile)
  }, [profile])

  const commonProps = useMemo(
    () => ({
      autoComplete: 'off',
      autoCorrect: 'off',
      fullWidth: true,
    }),
    [],
  )

  const handleProfileSelected = (content: string) => {
    localProfile.current = content

    setLocalProfileMessage('')
  }

  const [editor, setEditor] = useState({
    value: '',
    language: 'yaml',
  })

  const latestEditor = useLatest(editor)

  const editorMarks = useRef<editor.IMarker[]>([])
  const editorHasError = () =>
    editorMarks.current.length > 0 &&
    editorMarks.current.some((m) => m.severity === 8)

  const onSubmit = handleSubmit(async (form) => {
    if (editorHasError()) {
      message('Please fix the error before saving', {
        kind: 'error',
      })
      return
    }
    const toCreate = async () => {
      if (isRemote) {
        await createProfile(form)
      } else {
        if (localProfile.current) {
          await createProfile(form, localProfile.current)
        } else {
          // setLocalProfileMessage("Not selected profile");
          await createProfile(form, 'rules: []')
        }
      }
    }

    const toUpdate = async () => {
      const value = latestEditor.current.value
      await setProfileFile(form.uid, value)
      await setProfiles(form.uid, form)
    }

    try {
      if (isEdit) {
        await toUpdate()
      } else {
        await toCreate()
      }

      setTimeout(() => reset(), 300)

      onClose()
    } catch (err) {
      message('Failed to save profile: \n' + formatError(err), {
        kind: 'error',
      })
      console.error(err)
    }
  })

  const dialogProps = isEdit && {
    contentStyle: {
      overflow: 'hidden',
      padding: 0,
    },
    full: true,
  }

  const MetaInfo = useMemo(
    () => (
      <div className="flex flex-col gap-4 pt-2 pb-2">
        {!isEdit && (
          <SelectElement
            label={t('Type')}
            name="type"
            control={control}
            {...commonProps}
            size="small"
            required
            options={[
              {
                id: 'remote',
                label: t('Remote Profile'),
              },
              {
                id: 'local',
                label: t('Local Profile'),
              },
            ]}
          />
        )}

        <TextFieldElement
          label={t('Name')}
          name="name"
          control={control}
          size="small"
          fullWidth
          required
        />

        <TextFieldElement
          label={t('Descriptions')}
          name="desc"
          control={control}
          {...commonProps}
          size="small"
          multiline
        />

        {isRemote && (
          <>
            <TextFieldElement
              label={t('Subscription URL')}
              name="url"
              control={control}
              {...commonProps}
              size="small"
              multiline
              required
            />

            <TextFieldElement
              label={t('User Agent')}
              name="option.user_agent"
              control={control}
              {...commonProps}
              size="small"
              placeholder={`clash-nyanpasu/v${version}`}
            />

            <TextFieldElement
              label={t('Update Interval')}
              name="option.update_interval"
              control={control}
              {...commonProps}
              size="small"
              type="number"
              InputProps={{
                inputProps: { min: 0 },
                endAdornment: (
                  <InputAdornment position="end">mins</InputAdornment>
                ),
              }}
            />

            <Controller
              name="option.with_proxy"
              control={control}
              render={({ field }) => (
                <LabelSwitch
                  label={t('Use System Proxy')}
                  checked={field.value}
                  {...field}
                />
              )}
            />

            <Controller
              name="option.self_proxy"
              control={control}
              render={({ field }) => (
                <LabelSwitch
                  label={t('Use Clash Proxy')}
                  checked={field.value}
                  {...field}
                />
              )}
            />
          </>
        )}
        {!isRemote && !isEdit && (
          <>
            <ReadProfile
              key="read_profile"
              onSelected={handleProfileSelected}
            />

            {localProfileMessage && (
              <div className="ml-2 text-red-500">{localProfileMessage}</div>
            )}
            <span className="px-2 text-xs">
              * {t('Choose file to import or leave it blank to create new one')}
            </span>
          </>
        )}
      </div>
    ),
    [commonProps, control, isEdit, isRemote, localProfileMessage, t],
  )

  useAsyncEffect(async () => {
    if (profile) {
      reset(profile)
    }

    if (isEdit) {
      try {
        const value = await getProfileFile(profile?.uid)
        setEditor((editor) => ({ ...editor, value }))
      } catch (error) {
        console.error(error)
      }
    }
  }, [open])

  return (
    <BaseDialog
      title={isEdit ? t('Edit Profile') : t('Create Profile')}
      open={open}
      onClose={() => onClose()}
      onOk={onSubmit}
      divider
      {...dialogProps}
    >
      {isEdit ? (
        <div className="flex h-full">
          <div className="min-w-72 overflow-auto p-4">{MetaInfo}</div>

          <Divider orientation="vertical" />

          <Suspense fallback={null}>
            {open && (
              <ProfileMonacoViewer
                className="w-full"
                readonly={isRemote}
                schemaType="clash"
                value={editor.value}
                onChange={(value) =>
                  setEditor((editor) => ({ ...editor, value }))
                }
                onValidate={(marks) => (editorMarks.current = marks)}
                language={editor.language}
              />
            )}
          </Suspense>
        </div>
      ) : (
        MetaInfo
      )}
    </BaseDialog>
  )
}
