# peck

视觉 token 的落点。现在是**手写的占位**（`VERSION` 写 `placeholder`），变量名照 visual-atelier `schemes/peck/tokens/targets.json` 的 web 方言（`--color-*`），深色的值是现在界面在用的颜色。

- Phase 2 视觉定稿、站主打 tag `peck-v*` 后，visual-atelier 的 `sync` 把这个目录整个覆盖成同步来的副本（带 `VERSION` 的版本和提交、`SHA256SUMS` 校验和），之后**不要在这里手改**。
- 读取方式：`src/main.tsx` 先引 `peck.css`，再引 `src/index.css`；组件只用 CSS 变量。根元素的 `data-theme` 由 `src/lib/theme.ts` 写。
