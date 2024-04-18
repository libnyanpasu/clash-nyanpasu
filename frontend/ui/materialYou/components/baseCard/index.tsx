import { Box, Card, CardContent, Typography } from "@mui/material";
import { ReactNode } from "react";

export const BaseCard = ({
  label,
  labelChildren,
  children,
}: {
  label?: string;
  labelChildren?: ReactNode;
  children?: ReactNode;
}) => {
  return (
    <Card>
      <CardContent>
        {label && (
          <Box
            display="flex"
            justifyContent="space-between"
            alignItems="center"
          >
            <Typography sx={{ pb: 1 }} variant="h5" component="div">
              {label}
            </Typography>

            {labelChildren}
          </Box>
        )}

        {children}
      </CardContent>
    </Card>
  );
};
