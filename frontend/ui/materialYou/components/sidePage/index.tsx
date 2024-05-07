import { ReactNode, FC, memo, useMemo } from "react";
import Divider from "@mui/material/Divider";
import Toolbar from "@mui/material/Toolbar";
import Typography from "@mui/material/Typography";
import { BaseErrorBoundary } from "../basePage/baseErrorBoundary";
import style from "./style.module.scss";

interface Props {
  title?: ReactNode;
  header?: ReactNode;
  children?: ReactNode;
  sideBar?: ReactNode;
  side?: ReactNode;
  toolBar?: ReactNode;
}

const Header: FC<{ title?: ReactNode; header?: ReactNode }> = memo(
  function Header({
    title,
    header,
  }: {
    title?: ReactNode;
    header?: ReactNode;
  }) {
    return (
      <header style={{ userSelect: "none" }} data-windrag>
        <Typography variant="h4" component="h1" fontWeight={500} data-windrag>
          {title}
        </Typography>

        {header}
      </header>
    );
  },
);

export const SidePage: FC<Props> = ({
  title,
  header,
  children,
  sideBar,
  side,
  toolBar,
}) => {
  const memoizedChildren = useMemo(() => children, [children]);

  const memoizedSide = useMemo(() => side, [side]);

  return (
    <BaseErrorBoundary>
      <div className={style["MDYSidePage-Main"]} data-windrag>
        <Header title={title} header={header} />

        <div className={style["MDYSidePage-Container"]}>
          <div className={style["MDYSidePage-Layout"]}>
            <div className={style.LeftContainer}>
              {sideBar && <div>{sideBar}</div>}

              <div className={style["LeftContainer-Content"]}>
                <section>{memoizedSide}</section>
              </div>
            </div>

            <div className={style.RightContainer}>
              {toolBar && (
                <>
                  <Toolbar variant="dense">{toolBar}</Toolbar>

                  <Divider />
                </>
              )}

              <div
                className={style["RightContainer-Content"]}
                style={toolBar ? { height: "calc(100% - 49px)" } : undefined}
              >
                <section>{memoizedChildren}</section>
              </div>
            </div>
          </div>
        </div>
      </div>
    </BaseErrorBoundary>
  );
};
