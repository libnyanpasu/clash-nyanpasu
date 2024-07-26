import { motion } from "framer-motion";
import { FC, ReactNode } from "react";
import Divider from "@mui/material/Divider";
import Toolbar from "@mui/material/Toolbar";
import { BaseErrorBoundary } from "../basePage/baseErrorBoundary";
import Header from "../basePage/header";
import style from "./style.module.scss";

interface Props {
  title?: ReactNode;
  header?: ReactNode;
  children?: ReactNode;
  sideBar?: ReactNode;
  side?: ReactNode;
  sideClassName?: string;
  toolBar?: ReactNode;
  noChildrenScroll?: boolean;
  flexReverse?: boolean;
}

export const SidePage: FC<Props> = ({
  title,
  header,
  children,
  sideBar,
  side,
  sideClassName,
  toolBar,
  noChildrenScroll,
  flexReverse,
}) => {
  return (
    <BaseErrorBoundary>
      <div className={style["MDYSidePage-Main"]} data-windrag>
        <Header title={title} header={header} />

        <div className={style["MDYSidePage-Container"]}>
          <div
            className={style["MDYSidePage-Layout"]}
            style={{
              flexDirection: flexReverse ? "row-reverse" : undefined,
              gap: side ? undefined : "0px",
            }}
          >
            <motion.div
              className={style.LeftContainer}
              initial={false}
              animate={side ? "open" : "closed"}
              variants={{
                open: {
                  opacity: 1,
                  maxWidth: "348px",
                  minWidth: "192px",
                  display: "flex",
                },
                closed: {
                  opacity: 0.5,
                  maxWidth: 0,
                  transitionEnd: {
                    display: "none",
                  },
                },
              }}
            >
              {sideBar && <div>{sideBar}</div>}

              <div className={style["LeftContainer-Content"]}>
                <section className={sideClassName}>{side}</section>
              </div>
            </motion.div>

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
                <section
                  style={noChildrenScroll ? { overflow: "visible" } : undefined}
                >
                  {children}
                </section>
              </div>
            </div>
          </div>
        </div>
      </div>
    </BaseErrorBoundary>
  );
};
