import { LoadingButton } from "@mui/lab";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  type SxProps,
  type Theme,
} from "@mui/material";
import { TransitionProps } from "@mui/material/transitions";
import { AnimatePresence, motion } from "framer-motion";
import React, { ReactNode } from "react";
interface Props {
  title: ReactNode;
  open: boolean;
  okBtn?: ReactNode;
  okBtnDisabled?: boolean;
  cancelBtn?: ReactNode;
  disableOk?: boolean;
  disableCancel?: boolean;
  disableFooter?: boolean;
  contentSx?: SxProps<Theme>;
  children?: ReactNode;
  loading?: boolean;
  onOk?: () => void;
  onCancel?: () => void;
  onClose?: () => void;
}

export interface DialogRef {
  open: () => void;
  close: () => void;
}

export function BaseDialog(props: Props) {
  const {
    open,
    title,
    children,
    okBtn,
    okBtnDisabled,
    cancelBtn,
    contentSx,
    disableCancel,
    disableOk,
    disableFooter,
    loading,
  } = props;

  return (
    <Dialog
      className="123"
      open={open}
      onClose={props.onClose}
      keepMounted
      TransitionComponent={BaseDialogTransition}
    >
      <DialogTitle>{title}</DialogTitle>

      <DialogContent sx={contentSx}>{children}</DialogContent>

      {!disableFooter && (
        <DialogActions>
          {!disableCancel && (
            <Button variant="outlined" onClick={props.onCancel}>
              {cancelBtn}
            </Button>
          )}
          {!disableOk && (
            <LoadingButton
              disabled={loading || okBtnDisabled}
              loading={loading}
              variant="contained"
              onClick={props.onOk}
            >
              {okBtn}
            </LoadingButton>
          )}
        </DialogActions>
      )}
    </Dialog>
  );
}

const BaseDialogTransition = React.forwardRef(function BaseDialogTransition(
  props: TransitionProps,
  ref,
) {
  const { in: inProp, children } = props;

  return (
    <AnimatePresence>
      {inProp && (
        <motion.div
          style={{
            width: "fit-content",
            height: "fit-content",
            margin: "auto",
          }}
          initial={{ opacity: 0, scale: 0 }}
          animate={{
            opacity: 1,
            scale: 1,
          }}
          exit={{
            opacity: 0,
            scale: 0,
          }}
        >
          {children &&
            React.cloneElement(
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              React.Children.only(children as unknown as any),
              {
                style: {
                  opacity: 1,
                  visibility: "visible",
                },
                // TODO: 也许 framer motion 就不会产生这个，手动设定一下。等弄清楚了再说。
                tabIndex: -1,
                ref: ref,
              },
            )}
        </motion.div>
      )}
    </AnimatePresence>
  );
});
