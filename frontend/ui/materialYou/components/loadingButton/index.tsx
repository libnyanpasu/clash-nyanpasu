import { useControllableValue } from "ahooks";
import MuiLoadingButton, {
  LoadingButtonProps as MuiLoadingButtonProps,
} from "@mui/lab/LoadingButton";

export interface LoadingButtonProps extends MuiLoadingButtonProps {
  onClick: () => Promise<void> | void;
}

export const LoadingButton = ({
  loading,
  onClick,
  ...props
}: LoadingButtonProps) => {
  const [pending, setPending] = useControllableValue<boolean>(
    { loading },
    {
      defaultValue: false,
    },
  );

  const handleClick = async () => {
    if (onClick) {
      setPending(true);
      await onClick();
      setPending(false);
    }
  };

  return (
    <MuiLoadingButton onClick={handleClick} loading={pending} {...props} />
  );
};
