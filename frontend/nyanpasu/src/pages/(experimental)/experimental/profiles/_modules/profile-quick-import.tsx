import CloseSmallOutlineRounded from '~icons/material-symbols/close-small-outline-rounded'
import DownloadDoneRounded from '~icons/material-symbols/download-done-rounded'
import LinkRounded from '~icons/material-symbols/link-rounded'
import { Controller, useForm } from 'react-hook-form'
import { z } from 'zod'
import { Button } from '@/components/ui/button'
import { message } from '@/utils/notification'
import { zodResolver } from '@hookform/resolvers/zod'
import { cn } from '@nyanpasu/ui'

const formSchema = z.object({
  url: z.httpUrl(),
})

export default function ProfileQuickImport() {
  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: {
      url: '',
    },
  })

  const handleClear = () => {
    form.setValue('url', '')
  }

  const hasValue = Boolean(form.getValues('url')?.length)

  const handleSubmit = form.handleSubmit(
    (data) => {
      console.log(data)
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
    <form className="relative flex-1" onSubmit={handleSubmit}>
      <LinkRounded className="absolute top-1/2 left-3 size-6 -translate-y-1/2" />

      <Controller
        control={form.control}
        name="url"
        render={({ field }) => (
          <input
            className={cn(
              'bg-surface h-10 w-full rounded-full px-4 py-3',
              'text-sm outline-hidden',
              'pr-9 pl-11',
              'shadow-outline/30 shadow dark:shadow-none',
            )}
            type="text"
            placeholder="Import profile"
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            {...field}
          />
        )}
      />

      <Button
        icon
        className={cn(
          'absolute top-1/2 right-9 bottom-0 size-6 -translate-y-1/2',
          hasValue ? 'opacity-100' : 'opacity-0',
        )}
        onClick={handleClear}
        type="button"
      >
        <CloseSmallOutlineRounded className="size-6" />
      </Button>

      <Button
        icon
        className={cn(
          'absolute top-1/2 right-2 bottom-0 size-6 -translate-y-1/2',
          hasValue ? 'opacity-100' : 'opacity-0',
        )}
        type="submit"
      >
        <DownloadDoneRounded className="size-5" />
      </Button>
    </form>
  )
}
