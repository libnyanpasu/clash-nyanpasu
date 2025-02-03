import { useAsyncEffect, useReactive } from 'ahooks'
import { type editor } from 'monaco-editor'
import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { SelectElement, TextFieldElement, useForm } from 'react-hook-form-mui'
import { useTranslation } from 'react-i18next'
import { message } from '@/utils/notification'
import { Divider } from '@mui/material'
import {
  Profile,
  ProfileTemplate,
  useClash,
  useProfile,
  useProfileContent,
} from '@nyanpasu/interface'
import { BaseDialog, BaseDialogProps } from '@nyanpasu/ui'
import LanguageChip from './modules/language-chip'
import { getLanguage, ProfileType, ProfileTypes } from './utils'

const ProfileMonacoViewer = lazy(() => import('./profile-monaco-viewer'))

const formCommonProps = {
  autoComplete: 'off',
  autoCorrect: 'off',
  fullWidth: true,
}

const optionTypeMapping = [
  {
    id: 'js',
    value: ProfileTypes.JavaScript,
    language: 'javascript',
    label: 'JavaScript',
  },
  {
    id: 'lua',
    value: ProfileTypes.LuaScript,
    language: 'lua',
    label: 'LuaScript',
  },
  {
    id: 'merge',
    value: ProfileTypes.Merge,
    language: 'yaml',
    label: 'Merge',
  },
]

const convertTypeMapping = (data: Profile) => {
  optionTypeMapping.forEach((option) => {
    if (option.id === data.type) {
      data.type = option.value
    }
  })

  return data
}

export interface ScriptDialogProps extends Omit<BaseDialogProps, 'title'> {
  open: boolean
  onClose: () => void
  profile?: Profile
}

export const ScriptDialog = ({
  open,
  profile,
  onClose,
  ...props
}: ScriptDialogProps) => {
  const { t } = useTranslation()

  // const { getProfileFile, setProfileFile, createProfile, setProfiles } =
  //   useClash()

  const { create, update } = useProfile()

  const contentFn = useProfileContent(profile?.uid ?? '')

  const form = useForm<Profile>()

  const isEdit = Boolean(profile)

  useEffect(() => {
    if (isEdit) {
      form.reset(profile)
    } else {
      form.reset({
        type: 'merge',
        chain: [],
        name: t('New Script'),
        desc: '',
      })
    }
  }, [form, isEdit, profile, t])

  const [openMonaco, setOpenMonaco] = useState(false)

  const editor = useReactive<{
    value: string
    language: string
    rawType: ProfileType
  }>({
    value: ProfileTemplate.merge,
    language: 'yaml',
    rawType: 'merge',
  })

  const editorMarks = useRef<editor.IMarker[]>([])
  const editorHasError = () =>
    editorMarks.current.length > 0 &&
    editorMarks.current.some((m) => m.severity === 8)

  const onSubmit = form.handleSubmit(async (data) => {
    if (editorHasError()) {
      message(t('Please fix the error before submitting'), {
        kind: 'error',
      })
      return
    }

    convertTypeMapping(data)

    const editorValue = editor.value

    if (!editorValue) {
      return
    }

    try {
      if (isEdit) {
        await contentFn.upsert.mutateAsync(editorValue)
        await update.mutateAsync({
          uid: data.uid,
          profile: data,
        })
      } else {
        await create.mutateAsync({
          type: 'manual',
          data: {
            item: data,
            fileData: editorValue,
          },
        })
      }
    } finally {
      onClose()
    }
  })

  useAsyncEffect(async () => {
    if (isEdit) {
      await contentFn.query.refetch()

      editor.value = contentFn.query.data ?? ''
      editor.language = getLanguage(profile!.type)!
    } else {
      editor.value = ProfileTemplate.merge
      editor.language = 'yaml'
    }

    setOpenMonaco(open)
  }, [open])

  const handleTypeChange = () => {
    const data = form.getValues()

    editor.rawType = convertTypeMapping(data).type

    const lang = getLanguage(editor.rawType)

    if (!lang) {
      return
    }

    editor.language = lang

    switch (lang) {
      case 'yaml': {
        editor.value = ProfileTemplate.merge
        break
      }

      case 'lua': {
        editor.value = ProfileTemplate.luascript
        break
      }

      case 'javascript': {
        editor.value = ProfileTemplate.javascript
        break
      }
    }
  }

  return (
    <BaseDialog
      title={
        <div className="flex gap-2">
          <span>{isEdit ? t('Edit Script') : t('New Script')}</span>

          <LanguageChip
            type={isEdit ? (profile?.type ?? editor.rawType) : editor.rawType}
          />
        </div>
      }
      open={open}
      onClose={() => onClose()}
      onOk={onSubmit}
      divider
      contentStyle={{
        overflow: 'hidden',
        padding: 0,
      }}
      full
      {...props}
    >
      <div className="flex h-full">
        <div className="overflow-auto pt-4 pb-4">
          <div className="flex flex-col gap-4 pr-4 pb-4 pl-4">
            {!isEdit && (
              <SelectElement
                label={t('Type')}
                name="type"
                control={form.control}
                {...formCommonProps}
                size="small"
                required
                options={optionTypeMapping}
                onChange={() => handleTypeChange()}
              />
            )}

            <TextFieldElement
              label={t('Name')}
              name="name"
              control={form.control}
              {...formCommonProps}
              size="small"
              required
            />

            <TextFieldElement
              label={t('Descriptions')}
              name="desc"
              control={form.control}
              {...formCommonProps}
              size="small"
              multiline
            />
          </div>
        </div>

        <Divider orientation="vertical" />

        <Suspense fallback={null}>
          {openMonaco && (
            <ProfileMonacoViewer
              className="w-full"
              value={editor.value}
              onChange={(value) => {
                editor.value = value
              }}
              language={editor.language}
              onValidate={(marks) => {
                editorMarks.current = marks
              }}
              schemaType={
                editor.rawType === ProfileTypes.Merge ? 'merge' : undefined
              }
            />
          )}
        </Suspense>
      </div>
    </BaseDialog>
  )
}
