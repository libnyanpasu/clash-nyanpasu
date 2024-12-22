import { FC, memo, ReactNode } from 'react'

export const Header: FC<{ title?: ReactNode; header?: ReactNode }> = memo(
  function Header({
    title,
    header,
  }: {
    title?: ReactNode
    header?: ReactNode
  }) {
    return (
      <header className="select-none pl-2" data-tauri-drag-region>
        <h1 className="mb-1 text-4xl font-medium" data-tauri-drag-region>
          {title}
        </h1>

        {header}
      </header>
    )
  },
)

export default Header
