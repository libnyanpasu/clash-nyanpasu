import { FC, memo, ReactNode } from "react";

export const Header: FC<{ title?: ReactNode; header?: ReactNode }> = memo(
  function Header({
    title,
    header,
  }: {
    title?: ReactNode;
    header?: ReactNode;
  }) {
    return (
      <header className="select-none pl-2" data-windrag>
        <h1 className="mb-1 text-4xl font-medium" data-windrag>
          {title}
        </h1>

        {header}
      </header>
    );
  },
);

export default Header;
