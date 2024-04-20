import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from "@mui/material";
import LoadingButton from "@mui/lab/LoadingButton";
import { SxProps } from "@mui/material/styles";
import {
  cloneElement,
  forwardRef,
  ReactNode,
  useEffect,
  useState,
} from "react";
import { TransitionProps } from "@mui/material/transitions";
import { AnimatePresence, motion } from "framer-motion";
import React from "react";
import useDebounceFn from "ahooks/lib/useDebounceFn";

export interface BaseDialogProps {
  title: ReactNode;
  open: boolean;
  close?: string;
  ok?: string;
  disabledOk?: boolean;
  contentSx?: SxProps;
  children?: ReactNode;
  loading?: boolean;
  onOk?: () => void;
  onClose?: () => void;
}

export const BaseDialog = ({
  title,
  open,
  close,
  onClose,
  children,
  contentSx,
  disabledOk,
  loading,
  onOk,
  ok,
}: BaseDialogProps) => {
  const [mounted, setMounted] = useState(true);

  const { run: runMounted, cancel: cancelMounted } = useDebounceFn(
    () => setMounted(false),
    { wait: 300 },
  );

  const handleClose = () => {
    if (onClose) {
      onClose();
      runMounted();
    }
  };

  useEffect(() => {
    if (open) {
      setMounted(true);
      cancelMounted();
    }
  }, [open]);

  return (
    <Dialog
      open={open}
      onClose={handleClose}
      keepMounted={mounted}
      TransitionComponent={BaseDialogTransition}
    >
      <DialogTitle>{title}</DialogTitle>

      <DialogContent
        sx={{
          width: 400,
          ...contentSx,
        }}
      >
        {children}
      </DialogContent>

      <DialogActions>
        {onClose && (
          <Button variant="outlined" onClick={handleClose}>
            {close}
          </Button>
        )}

        {onOk && (
          <LoadingButton
            disabled={loading || disabledOk}
            loading={loading}
            variant="contained"
            onClick={onOk}
          >
            {ok}
          </LoadingButton>
        )}
      </DialogActions>
    </Dialog>
  );
};

const BaseDialogTransition = forwardRef(function BaseDialogTransition(
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
            maxHeight: "100vh",
            position: "fixed",
          }}
          initial={{
            opacity: 0,
            scale: 0,
            top: "50%",
            left: "50%",
            translateX: "-50%",
            translateY: "-50%",
          }}
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
            cloneElement(
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
