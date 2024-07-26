import clsx from "clsx";
import { useTheme } from "@mui/material";
import styles from "./index.module.scss";

export type Props = React.DetailedHTMLProps<
  React.HTMLAttributes<HTMLElement>,
  HTMLElement
>;

export default function Kbd({ className, children, ...rest }: Props) {
  const theme = useTheme();
  return (
    <kbd
      className={clsx(
        styles.kbd,
        theme.palette.mode === "dark" && styles.dark,
        className,
      )}
      {...rest}
    >
      {children}
    </kbd>
  );
}
