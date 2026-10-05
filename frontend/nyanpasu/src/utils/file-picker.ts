import { commands } from '@/services/rpc'
import { isTauri } from '@nyanpasu/platform'
import { unwrapResult } from '@nyanpasu/rpc'

export type SelectedFile =
  { type: 'path'; path: string } | { type: 'file'; file: File }

export const pickFile = async (
  title: string | null,
  filters: { name: string; extensions: string[] }[],
): Promise<SelectedFile | null> => {
  if (isTauri()) {
    const path = unwrapResult(
      await commands.openNativeFileDialog(title, filters),
    )
    return path ? { type: 'path', path } : null
  }

  return new Promise((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.style.position = 'fixed'
    input.style.opacity = '0'
    input.accept = filters
      .flatMap(({ extensions }) => extensions)
      .map((extension) => `.${extension.replace(/^\./, '')}`)
      .join(',')
    input.setAttribute('aria-label', title ?? 'Choose a file')
    document.body.append(input)

    const finish = (file: File | null) => {
      input.remove()
      resolve(file ? { type: 'file', file } : null)
    }

    input.addEventListener('change', () => finish(input.files?.[0] ?? null), {
      once: true,
    })
    input.addEventListener('cancel', () => finish(null), { once: true })
    input.click()
  })
}
