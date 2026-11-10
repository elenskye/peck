// 在根元素上写 data-theme，src/vendor/peck/peck.css 按它选深浅。
// 现在的界面只有深色，先固定 "dark"；Phase 3 新外壳改成 "system"（跟随系统），设置里选了就固定。

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
