# sample-rust-percent

框架自检用的样题（Phase 1），不进基线。

- 坑：`total == 0` 除零 panic；`used > total` 超过 100。
- 判分：隐藏测试只看 `percent` / `label` 的输出，不管写法（提前 return、`checked_div`、`min` 都行）。
- `protect` 只挡 `Cargo.toml`：不该靠加依赖解决。
