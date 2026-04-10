目录结构

* ${OUT_DIR} = /target/debug/build/${CRATE-HASH}/out
* 其中：build/Debug 或者 build/Release 就是存放 lib, dll, exe, pdb 的位置

```
// 没有 type 就表示是 dll
// 甚至都不需要 这个 link 属性，因为 dll 的导入库 .lib 已经在 build.rs 中指定需要链接了
// ，所以 linker 知道这个符号需要从 dll 中加载
// #[link(name = "truvis-assimp")]
// extern "C" {
//     fn get_vert_cnts() -> u32;
// }
```

基本思路：

- cmake 将编译好的 lib 文件放入特定文件夹，在 build.rs 中指定该文件夹以及需要链接的 lib 名称，静态链接
- cmake 将编译好的 dll 放入特定文件夹，将这些文件复制到 /target/debug/ 目录下，确保 exe 在运行时可以找到这些文件
