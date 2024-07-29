import { ReactNode } from "react";
import { classNames } from "@/utils";
import { Public } from "@mui/icons-material";

export interface ContentDisplayProps {
  className?: string;
  message?: string;
  children?: ReactNode;
}

export const ContentDisplay = ({
  message,
  children,
  className,
}: ContentDisplayProps) => (
  <div
    className={classNames(
      "flex h-full w-full items-center justify-center",
      className,
    )}
  >
    <div className="flex flex-col items-center gap-4">
      {children ? (
        children
      ) : (
        <>
          <Public className="!size-16" />

          <b>{message}</b>
        </>
      )}
    </div>
  </div>
);

export default ContentDisplay;
