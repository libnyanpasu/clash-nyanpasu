import CloseSmallOutlineRounded from '~icons/material-symbols/close-small-outline-rounded'
import DownloadRounded from '~icons/material-symbols/download-rounded'
import LinkRounded from '~icons/material-symbols/link-rounded'
import { useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { useProfile } from '@nyanpasu/interface'
import { cn } from '@nyanpasu/ui'

const formSchema = z.object({
  url: z.httpUrl(),
})

export default function ProfileQuickImport() {
  const { create } = useProfile()

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      url: '',
    },
  })

  const handleClear = () => {
    form.reset({ url: '' })
  }

  const hasValue = form.watch('url')

  const [loading, setLoading] = useState(false)

  const handleSubmit = form.handleSubmit(
    async (data) => {
      try {
        setLoading(true)

        await create.mutateAsync({
          type: 'url',
          data: {
            url: data.url,
            option: null,
          },
        })

        handleClear()

        message(m.profile_quick_import_success_message(), {
          title: 'Import profile',
          kind: 'info',
        })
      } catch (error) {
        console.error(error)
      } finally {
        setLoading(false)
      }
    },
    (errors) => {
      console.error(errors)
      message(errors.url?.message ?? '', {
        title: 'Import profile failed',
        kind: 'error',
      })
    },
  )

  return (
    <form
      className={cn(
        'relative flex flex-1 items-center gap-1',
        'bg-surface h-10 w-full rounded-full pr-1 pl-3',
        'shadow-outline/30 shadow dark:shadow-none',
      )}
      onSubmit={handleSubmit}
    >
      <LinkRounded className="size-6" />

      <Controller
        control={form.control}
        name="url"
        render={({ field }) => (
          <input
            className="h-full flex-1 px-1 text-sm outline-hidden"
            type="text"
            placeholder={m.profile_quick_import_placeholder()}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            {...field}
          />
        )}
      />

      {hasValue && (
        <>
          {!loading && (
            <Button icon className="size-8" onClick={handleClear} type="button">
              <CloseSmallOutlineRounded className="size-6" />
            </Button>
          )}

          <Button icon className="size-8" type="submit" loading={loading}>
            <DownloadRounded className="size-6" />
          </Button>
        </>
      )}
    </form>
  )
}
