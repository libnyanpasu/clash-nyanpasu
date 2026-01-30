import { cva, type VariantProps } from 'class-variance-authority'
import {
  ChangeEvent,
  ComponentProps,
  createContext,
  DragEvent,
  ReactNode,
  RefObject,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react'
import getSystem from '@/utils/get-system'
import { cn } from '@nyanpasu/ui'
import { readTextFile } from '@tauri-apps/plugin-fs'

const isWin = getSystem() === 'windows'

const FileDropZoneContext = createContext<{
  isDragging: boolean
  isLoading: boolean
  fileName: string | null
  accept: string[]
  disabled: boolean
  fileInputRef: RefObject<HTMLInputElement | null>
  handleClick: () => void
} | null>(null)

const useFileDropZoneContext = () => {
  const context = useContext(FileDropZoneContext)

  if (!context) {
    throw new Error('FileDropZone components must be used within FileDropZone')
  }

  return context
}

export const fileDropZoneVariants = cva(
  [
    'relative flex min-h-24 flex-col items-center justify-center gap-2',
    'rounded-md border border-dashed p-4',
    'transition-colors duration-200',
    'cursor-pointer',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary',
  ],
  {
    variants: {
      variant: {
        default: [
          'border-outline-variant',
          'bg-transparent',
          'hover:border-primary/50',
          'hover:bg-surface-variant/30',
        ],
        outline: [
          'border-outline-variant',
          'bg-surface dark:bg-surface',
          'hover:border-primary/50',
          'hover:bg-surface-variant/30',
        ],
      },
      isDragging: {
        true: 'border-primary bg-primary-container/20',
        false: '',
      },
      disabled: {
        true: 'cursor-not-allowed opacity-50',
        false: '',
      },
    },
    compoundVariants: [
      {
        disabled: true,
        className: 'hover:border-outline-variant hover:bg-transparent',
      },
    ],
    defaultVariants: {
      variant: 'default',
      isDragging: false,
      disabled: false,
    },
  },
)

export type FileDropZoneVariants = VariantProps<typeof fileDropZoneVariants>

export interface FileDropZoneProps
  extends
    Omit<
      ComponentProps<'div'>,
      | 'onChange'
      | 'onDragEnter'
      | 'onDragLeave'
      | 'onDragOver'
      | 'onDrop'
      | 'onClick'
    >,
    FileDropZoneVariants {
  value?: string | null
  onChange?: (filePath: string) => void
  onFileRead?: (content: string) => void
  accept: string[]
  disabled?: boolean
  fileSelected?: (fileName: string) => ReactNode
}

export function FileDropZone({
  value,
  onChange,
  onFileRead,
  accept,
  className,
  disabled = false,
  variant,
  fileSelected,
  children,
  ...props
}: FileDropZoneProps) {
  const [isDragging, setIsDragging] = useState(false)

  const [isLoading, setIsLoading] = useState(false)

  const [fileName, setFileName] = useState<string | null>(
    value
      ? ((isWin ? value.split('\\').at(-1) : value.split('/').at(-1)) ?? null)
      : null,
  )
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Update fileName when value changes
  useEffect(() => {
    if (value) {
      const name = isWin ? value.split('\\').at(-1) : value.split('/').at(-1)
      setFileName(name || null)
    } else {
      setFileName(null)
    }
  }, [value])

  const handleFile = async (filePath: string, file?: File) => {
    if (disabled) return

    try {
      setIsLoading(true)

      let content: string

      // If file object is provided (from drag & drop), use FileReader
      // Otherwise, use Tauri's readTextFile API
      if (file) {
        content = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader()
          reader.onload = (e) => {
            resolve(e.target?.result as string)
          }
          reader.onerror = reject
          reader.readAsText(file)
        })
      } else {
        content = await readTextFile(filePath)
      }

      // Read file content if callback is provided
      if (onFileRead) {
        onFileRead(content)
      }

      // Update file path
      onChange?.(filePath)

      // Extract file name
      const name =
        file?.name ||
        (isWin ? filePath.split('\\').at(-1) : filePath.split('/').at(-1))
      setFileName(name || null)
    } catch (error) {
      console.error('Failed to read file:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const handleDragEnter = (e: DragEvent<HTMLDivElement>) => {
    if (disabled) return
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(true)
  }

  const handleDragLeave = (e: DragEvent<HTMLDivElement>) => {
    if (disabled) return
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(false)
  }

  const handleDragOver = (e: DragEvent<HTMLDivElement>) => {
    if (disabled) return
    e.preventDefault()
    e.stopPropagation()
  }

  const handleDrop = async (e: DragEvent<HTMLDivElement>) => {
    if (disabled) return
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(false)

    const files = e.dataTransfer.files
    if (files.length === 0) return

    const file = files[0]

    // Check file extension
    const fileExt = file.name
      .toLowerCase()
      .substring(file.name.lastIndexOf('.'))
    if (!accept.some((ext) => fileExt === ext.toLowerCase())) {
      console.error('File type not accepted')
      return
    }

    // In Tauri, try to get file path from the file object
    // If not available, use FileReader API
    const filePath = (file as File & { path?: string }).path as
      | string
      | undefined

    if (filePath) {
      // File path is available (Tauri native drag & drop)
      await handleFile(filePath, file)
    } else {
      // Fallback: use file name as identifier and read content via FileReader
      // Note: In this case, we use the file name as the path identifier
      await handleFile(file.name, file)
    }
  }

  const handleClick = () => {
    if (disabled || isLoading) {
      return
    }

    fileInputRef.current?.click()
  }

  const handleFileInputChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return

    const file = files[0]

    // In Tauri, file input may have path property
    const filePath = (file as File & { path?: string }).path as
      | string
      | undefined

    if (filePath) {
      // File path is available (Tauri file dialog)
      await handleFile(filePath)
    } else {
      // Fallback: use file name and read via FileReader
      await handleFile(file.name, file)
    }

    // Reset input
    if (fileInputRef.current) {
      fileInputRef.current.value = ''
    }
  }

  return (
    <FileDropZoneContext.Provider
      value={{
        isDragging,
        isLoading,
        fileName,
        accept,
        disabled,
        fileInputRef,
        handleClick,
      }}
    >
      <div
        className={cn(
          fileDropZoneVariants({
            variant,
            isDragging,
            disabled,
          }),
          className,
        )}
        data-slot="file-drop-zone"
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onClick={handleClick}
        {...props}
      >
        <input
          data-slot="file-drop-zone-input"
          ref={fileInputRef}
          type="file"
          accept={accept.join(',')}
          className="hidden"
          onChange={handleFileInputChange}
          disabled={disabled}
        />

        {children}

        {/* {isLoading ? (
          <FileDropZoneLoading />
        ) : fileName ? (
          (fileSelected?.(fileName) ?? (
            <FileDropZoneFileSelected name={fileName} />
          ))
        ) : (
          <FileDropZonePlaceholder accept={accept} />
        )} */}
      </div>
    </FileDropZoneContext.Provider>
  )
}

export function FileDropZoneLoading(props: ComponentProps<'div'>) {
  const { isLoading } = useFileDropZoneContext()

  if (!isLoading) {
    return null
  }

  return <div data-slot="file-drop-zone-loading" {...props} />
}

export function FileDropZonePlaceholder(props: ComponentProps<'div'>) {
  const { isLoading, fileName } = useFileDropZoneContext()

  if (isLoading || fileName) {
    return null
  }

  return <div data-slot="file-drop-zone-placeholder" {...props} />
}

export function FileDropZoneFileSelected(props: ComponentProps<'div'>) {
  const { fileName } = useFileDropZoneContext()

  if (!fileName) {
    return null
  }

  return <div data-slot="file-drop-zone-file-selected" {...props} />
}
