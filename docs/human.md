** 该文档由人类维护，agent 禁止修改！！！！！ **

## Asset 部分

- gltf 之类的导入，可以叫做 scene import。这可以做成一个单独的模块。
  导入的关键部分，决定导入是否成功的部分，是 mesh，texture 以及 material 资源是否成功安装。
  安装成功后，就可以用 raw scene 结构去往场景里面放了。（包括 node 信息，各种资源信息）

## 资源依赖与缺失的处理

首先澄清一些概念：

- MaterialTable：由 ResourceSystem 拥有与维护，表示所有的 Material 参数信息，会被上传到 GPU 上。
  其中每个 Material 对应的 slot 是稳定的
- MaterialSlotTable：由 RenderWorld 拥有并维护，存放 world 内 instance 引用的材质的 slot 的信息，属于一个间接表。

以下是资源依赖与缺失时的处理：

- Instance 一旦被加入到场景中，只要 Instance 所依赖的 MeshAsset 准备好了，就将这个 Instance 加入到 GPUScene 以及 TLAS
  Scene 中。
  Instance 依赖的 MaterialAsset 一定可以通过 ResourceSystem 查询到对应的 slot，因此 MaterialSlotTable 可以正常填写
- Material 一旦被加入到 ResourceSystem 中，就为其分配一个 MaterialTable 的稳定 slot。
  因此 RenderWorld 在查询 MaterialAsset 时，一定可以得到一个 slot。由于可以保证 MaterialTable 会在当前帧更新，
  且设置好了同步保证 pass 会依赖 MaterialTable 的更新，因此 pass 访问具体 Material 时，一定可以访问到有效的信息。
- Material 信息在更新到 MaterialTable 时，材质参数可以直接更新进去，
  如果还依赖 Texture，在查询到 Texture 并没有准备好后，可以在 MaterialTable 的材质参数的 Texture 的位置填写 null binding，
  当作没有 Texture 处理。
- 由 ResourceSystem 负责每帧检查等待 Texture 的 Material，并且在 Texture 准备好后，
  将 Texture 对应的 slot 更新到 MaterialTable 中