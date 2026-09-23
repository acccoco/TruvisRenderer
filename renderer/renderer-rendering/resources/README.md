# SDR 后处理资源

AgX default-contrast LUT 来源：Bevy commit `566358363126dd69f6e457e47f306c68f8041d2a`，
[`AgX-default_contrast.ktx2`](https://raw.githubusercontent.com/bevyengine/bevy/566358363126dd69f6e457e47f306c68f8041d2a/crates/bevy_core_pipeline/src/tonemapping/luts/AgX-default_contrast.ktx2)。
配套公式位于同版本 `tonemapping_shared.wgsl`。保留上游 MIT/Apache-2.0 许可文本。

- 原始 KTX2 SHA-256：`90fcdff22741698dbed7b3b2fd3133006847c63185d5dc42fbab6084f50e69cf`。
- atlas SHA-256：`dd611f3adfdbb8cdb136015c0a179aad495dc37428d73c5c529682e26f7a69d5`。
- 原始格式为 32³ RGBA16F、Zstd supercompression。读取 level 0 index，Zstd 解包得到 little-endian RGBA16F。
- 无损重排：原偏移 `((b*32+g)*32+r)*8` → atlas 偏移 `(g*1024+b*32+r)*8`，不变换颜色、不重新量化。
- atlas 尺寸 1024×32，RGBA16F 线性 sampled image；各 B slice 做双线性采样，再沿 B 插值。
- `SdrPostProcess` 唯一拥有 LUT 和曝光历史，所有资源在现有 RenderThread 生命周期内创建/释放。
