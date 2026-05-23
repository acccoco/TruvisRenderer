# Engine Utils

`engine/utils/` 存放引擎内部工具 crate，区别于根目录 `tools/` 中的外部工具资源。

当前包含：

- `truvis-path`：基于根目录 `map.toml` 的工作区路径管理，提供 `engine/`、`assets/`、
  `assets/resources/`、`tools/`、`target/`、`.temp/`、`engine/shader/` 和 `engine/cxx/` 等统一入口。
- `truvis-fetch-res`：读取根目录 `resources.toml`，下载模型资产与外部工具资源，并解压或复制到配置中的目标目录。

`truvis-fetch-res` 的可执行入口名是 `fetch_res`；推荐通过根目录 `just fetch-res` 调用。
