import globalClassNames from "../style.d";
declare const classNames: typeof globalClassNames & {
  readonly oops: "oops";
  readonly dark: "dark";
};
export = classNames;
