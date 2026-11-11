// 在根元素上写 data-theme，src/vendor/peck/peck.css 按它选深浅。
// 默认 "system"（跟随系统）；设置的「外观」里选了深色或浅色就固定。

export type ThemeChoice = "system" | "light" | "dark";

const media = () => window.matchMedia("(prefers-color-scheme: dark)");

function write(choice: ThemeChoice) {
  const dark = choice === "dark" || (choice === "system" && media().matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

let unlisten: (() => void) | null = null;

export function applyTheme(choice: ThemeChoice) {
  unlisten?.();
  unlisten = null;
  write(choice);
  if (choice === "system") {
    const query = media();
    const onChange = () => write("system");
    query.addEventListener("change", onChange);
    unlisten = () => query.removeEventListener("change", onChange);
  }
}
