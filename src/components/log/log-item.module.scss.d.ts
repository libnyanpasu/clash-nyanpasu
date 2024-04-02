import globalClassNames from "../../style.d";
declare const classNames: typeof globalClassNames & {
  readonly item: "item";
  readonly shiki: "shiki";
  readonly dark: "dark";
};
export = classNames;
