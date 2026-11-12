// Vite 的 `?raw` 导入：文件原文当字符串（假数据读仓库里的夹具用）。
declare module "*?raw" {
  const text: string;
  export default text;
}
