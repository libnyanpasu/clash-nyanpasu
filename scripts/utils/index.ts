import { GITHUB_PROXY } from "./env";

export const getGithubUrl = (url: string) => {
  return new URL(url.replace(/^https?:\/\//g, ""), GITHUB_PROXY).toString();
};

export const array2text = (
  array: string[],
  type: "newline" | "space" = "newline",
): string => {
  let result = "";

  const getSplit = () => {
    if (type == "newline") {
      return "\n";
    } else if (type == "space") {
      return " ";
    }
  };

  array.forEach((value, index) => {
    if (index === array.length - 1) {
      result += value;
    } else {
      result += value + getSplit();
    }
  });

  return result;
};
