import { ReactNode, FC, CSSProperties, memo } from "react";
import Typography from "@mui/material/Typography";
import { BaseErrorBoundary } from "./baseErrorBoundary";
import "./style.scss";

interface Props {
  title?: ReactNode;
  header?: ReactNode;
  contentStyle?: CSSProperties;
  sectionStyle?: CSSProperties;
  full?: boolean;
  children?: ReactNode;
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

export const BasePage: FC<Props> = ({
  title,
  header,
  contentStyle,
  sectionStyle,
  full,
  children,
}) => {
  return (
    <BaseErrorBoundary>
      <div className="MDYBasePage" data-windrag>
        <Header title={title} header={header} />

        <div className={`MDYBasePage-container ${full ? "no-padding" : ""}`}>
          <section style={sectionStyle}>
            <div className="MDYBasePage-content" style={contentStyle}>
              {children}
            </div>
          </section>
        </div>
      </div>
    </BaseErrorBoundary>
  );
};
