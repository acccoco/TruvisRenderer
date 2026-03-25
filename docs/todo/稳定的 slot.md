## Bindless Manager

更改列表的 `Map`  应该怎么设计：

- key: 具体的 slot 号
- value: 资源的 GUID，以及 remain frames（和 fif 有关）

为什么 key 不是 GUID？如果是 GUID 的话，还需要根据 GUID 去查询 slot 号。而且这里本来就是跟踪 GPU 上面的 buffer 的变动，只需要关注
slot 就好了。


---

## Resource 同步到 GPU
- 资源系统可以在资源创建时就分配bindless id
- cpu scene 根据资源id 去获取bindless id，然后将bindless id 放入材质里面即可。
- 资源加载，上传完成后，直接更新bindless descriptor set即可，就不需要建立instance到资源的依赖了.



## TODO：GPU Scene 场景同步

这个设计思路最好归档到文档中
- mat slot buffer和bindless 的大小是固定的
- cpu使用empty list 来管理空位
- cpu里面每个 slot 记录最后更新 frame。向 gpu buffer 同步时，遍历检查每个slot是否需要更新。
- 可以增加一个标记，标识整个场景是否发生更新，来优化性能。大多数情况下场景都没有更新，因此可以跳过遍历。
- 关于增量更新：可以用以一个单独的 Map 来记录 CPU 的 dirty entry，并且在 entry 里面记录已经有哪个 GPU 帧被更新了（比如 0-3 的计数器）。这样做的好处是：不会污染原始数据。
- 关键点：额外的数据记录的是：当前 CPU 的数据和【前3帧】GPU 数据之间的差异。确保更新之后的 GPU 数据可以和当前 CPU 数据保持一直，且更新开销较小。

绘图表示：
```text
FRAME   CPU     OP
    1    state1   op1     
    2    state2   op2
# state3 = state0 + op1 + op2 + op3
```
实际上额外的数据，就记录着 op1-3
