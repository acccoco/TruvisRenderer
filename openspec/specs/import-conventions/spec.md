## Purpose

定义 Rust 源文件中 `use` 语句的分组与排序约定，确保整个 workspace 内的 import 风格一致、可机械化验证，并降低 diff 噪声。

## Requirements

### Requirement: use 语句 SHALL 按四组排列并以空行分隔

每个 Rust 源文件的顶层 `use` 区域 SHALL 按以下四组依次排列，组与组之间以恰好一行空行分隔。

```
// 1. std / core / alloc
use std::collections::HashMap;
use std::path::PathBuf;

// 2. 外部 crate（非 workspace 成员）
use ash::vk;
use itertools::Itertools;

// 3. workspace 内其他 crate（truvis_* 等）
use truvis_gfx::gfx::Gfx;
use truvis_render_interface::frame_counter::FrameCounter;

// 4. 当前 crate 内部（crate:: / self:: / super::）
use crate::render_pipeline::rt_render_graph::RtPipeline;
```

#### Scenario: 四组顺序正确

- **WHEN** 一个 Rust 源文件包含来自多个组的 `use` 语句
- **THEN** SHALL 按 std → 外部 crate → workspace crate → crate 内部的顺序排列
- **AND** 组与组之间 SHALL 有且仅有一行空行

#### Scenario: 单组内按字母序排列

- **WHEN** 同一组内存在多条 `use` 语句
- **THEN** SHALL 按模块路径的字典序（ASCII 升序）排列

#### Scenario: 缺失的组可省略

- **WHEN** 某个文件不需要某一组的 import（例如不使用 std）
- **THEN** 该组 SHALL 整体省略，不保留空行占位
- **AND** 相邻存在的组之间仍保持一行空行分隔

### Requirement: 组内多路径 use 树 SHALL 优先展开为独立行

当同一父模块下仅导入少量符号时，SHALL 优先使用独立 `use` 行而非嵌套 `use` 树。

#### Scenario: 少量导入使用独立行

- **WHEN** 从同一父模块导入 1-2 个符号
- **THEN** SHALL 使用独立 `use` 行（如 `use ash::vk;`）

#### Scenario: 多符号导入可使用 use 树

- **WHEN** 从同一父模块导入 3 个及以上符号
- **THEN** MAY 使用 `use parent::{A, B, C};` 形式
- **AND** 花括号内符号 SHALL 按字母序排列

### Requirement: glob import SHALL 仅用于明确的惯用场景

`use some::module::*` 形式 SHALL 仅限于以下惯用场景，避免命名空间污染。

#### Scenario: 允许的 glob import

- **WHEN** 在 prelude 模式、trait 方法批量引入、或测试模块中使用 glob import
- **THEN** SHALL 被视为合规

#### Scenario: 禁止的 glob import

- **WHEN** 在普通业务模块中使用 glob import 引入非 prelude 内容
- **THEN** SHALL 替换为显式导入路径
