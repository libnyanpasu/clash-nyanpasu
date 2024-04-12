import { cloneElement, isValidElement, ReactNode, useRef } from "react";
import noop from "@/utils/noop";

interface Props<Value> {
  value?: Value;
  valueProps?: string;
  loading?: boolean;
  onChangeProps?: string;
  waitTime?: number;
  onFormat?: (...args: any[]) => Value;
  onGuard?: (value: Value) => Promise<void>;
  onCatch?: (error: Error) => void;
  children: ReactNode;
}

export function GuardState<T>(props: Props<T>) {
  const {
    value,
    children,
    valueProps = "value",
    loading,
    onChangeProps = "onChange",
    onGuard = noop,
    onCatch = noop,
    onFormat = (v: T) => v,
  } = props;

  if (!isValidElement(children)) {
    return children as any;
  }

  const childProps = { ...children.props, loading };

  childProps[valueProps] = value;

  childProps[onChangeProps] = async (...args: any[]) => {
    try {
      const newValue = (onFormat as any)(...args);

      await onGuard(newValue);
    } catch (err: any) {
      onCatch(err);
    }
  };

  return cloneElement(children, childProps);
}
