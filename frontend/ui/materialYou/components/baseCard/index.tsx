import { motion } from "framer-motion";
import { ReactNode } from "react";
import {
  alpha,
  Box,
  Card,
  CardContent,
  CircularProgress,
  Typography,
  useTheme,
} from "@mui/material";
import style from "./style.module.scss";

export const BaseCard = ({
  label,
  labelChildren,
  loading,
  children,
}: {
  label?: string;
  labelChildren?: ReactNode;
  loading?: boolean;
  children?: ReactNode;
}) => {
  const { palette } = useTheme();

  return (
    <Card style={{ position: "relative" }}>
      <CardContent>
        {label && (
          <Box
            display="flex"
            justifyContent="space-between"
            alignItems="center"
            sx={{ pb: 1 }}
          >
            <Typography variant="h5" component="div">
              {label}
            </Typography>

            {labelChildren}
          </Box>
        )}

        {children}
      </CardContent>

      <motion.div
        initial={false}
        animate={loading ? "loading" : "none"}
        variants={{
          loading: { opacity: 1, visibility: "visible" },
          none: {
            opacity: 0,
            transitionEnd: {
              visibility: "hidden",
            },
          },
        }}
      >
        <div
          className={style.LoadingMask}
          style={{
            backgroundColor: alpha(palette.grey[100], 0.1),
          }}
        >
          <CircularProgress />
        </div>
      </motion.div>
    </Card>
  );
};
