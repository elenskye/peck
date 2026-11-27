# sample-ts-slug

框架自检用的样题（Phase 1），不进基线。

- 坑：`String.replace` 只换第一个；连着的空格、前后空格。
- 判分：隐藏测试只看输出。正则、`split` + `join`、`replaceAll` 都算对；制表符怎么处理不考。
- `protect` 挡原来的测试和 `package.json`：不能靠改测试过。
