import { CSSProperties, FC, ReactNode, useEffect, useRef } from "react";
import { BaseErrorBoundary } from "./baseErrorBoundary";
import Header from "./header";
import "./style.scss";

interface Props {
  title?: ReactNode;
  header?: ReactNode;
  contentStyle?: CSSProperties;
  sectionStyle?: CSSProperties;
  full?: boolean;
  children?: ReactNode;
}

export const BasePage: FC<Props> = ({
  title,
  header,
  contentStyle,
  sectionStyle,
  full,
  children,
}) => {
  const sectionStyleRef = useRef(sectionStyle);
  const contentStyleRef = useRef(contentStyle);

  useEffect(() => {
    sectionStyleRef.current = sectionStyle;
    contentStyleRef.current = contentStyle;
  }, [sectionStyle, contentStyle]);

  return (
    <BaseErrorBoundary>
      <div className="MDYBasePage" data-windrag>
        <Header title={title} header={header} />

        <div className={`MDYBasePage-container ${full ? "no-padding" : ""}`}>
          <div className="MDYBasePage-content" style={contentStyleRef.current}>
            <section style={sectionStyleRef.current}>{children}</section>
          </div>
        </div>
      </div>
    </BaseErrorBoundary>
  );
};
