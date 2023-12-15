import { classNames } from "@/utils";
import { formatAnsi } from "@/utils/shiki";
import { Box, styled, useTheme } from "@mui/material";
import React, { useEffect, useState } from "react";
import styles from "./log-item.module.scss";

const Item = styled(Box)(({ theme: { palette, typography } }) => ({
  padding: "8px 0",
  margin: "0 12px",
  lineHeight: 1.35,
  borderBottom: `1px solid ${palette.divider}`,
  fontSize: "0.875rem",
  fontFamily: typography.fontFamily,
  userSelect: "text",
  "& .time": {
    color: palette.text.secondary,
    fontWeight: "thin",
  },
  "& .type": {
    display: "inline-block",
    marginLeft: 8,
    textAlign: "center",
    borderRadius: 2,
    textTransform: "uppercase",
    fontWeight: "600",
  },
  '& .type[data-type="error"], & .type[data-type="err"]': {
    color: palette.error.main,
  },
  '& .type[data-type="warning"], & .type[data-type="warn"]': {
    color: palette.warning.main,
  },
  '& .type[data-type="info"], & .type[data-type="inf"]': {
    color: palette.info.main,
  },
  "& .data": {
    color: palette.text.primary,
  },
}));

interface Props {
  value: ILogItem;
}

const LogItem = (props: Props) => {
  const theme = useTheme();
  const { value } = props;
  const [payload, setPayload] = useState(value.payload);
  useEffect(() => {
    formatAnsi(value.payload).then((res) => {
      setPayload(res);
    });
  }, [value.payload]);

  return (
    <Item>
      <div>
        <span className="time">{value.time}</span>
        <span className="type" data-type={value.type.toLowerCase()}>
          {value.type}
        </span>
      </div>
      <div
        style={
          {
            "--item-font": theme.typography.fontFamily as string,
          } as React.CSSProperties
        }
      >
        <span
          className={classNames(styles.item, "data")}
          dangerouslySetInnerHTML={{
            __html: payload,
          }}
        />
      </div>
    </Item>
  );
};

export default LogItem;
