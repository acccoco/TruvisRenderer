## TL;DR

当前 BindlessManager 的设计，使得 image view 对应的 Bindless descriptor 的 slot 是稳定的。

- 首先需要在 BindlessManager 中维护最新的 slots 情况：使用 vector 来跟踪，vector 内存放的就是 GfxImageViewHandle。这是
  BindlessManager 的核心结构
- 然后还需要从 GfxImageViewHandle -> slot 的反向映射，用于查询；这是辅助的数据结构。
- 因为存在 fif，GPU 上面的多个 descriptor set 之间，以及和 CPU 中的 slot 内容，都不是一致的，因此需要维护一个 dirty 列表。
- dirty 列表的 key 应该是 slot，value 里面需要存放发生更改的 frame id。GPU 上面的 descriptor set 就根据这个 dirty
  信息进行增量更新。

## 数据结构

`BindlessManager` 的字段分为三层：

**核心（主数据）**：固定容量 Vec，index = shader 访问的 slot 号

```rust
srvs_slots: Vec<Option<GfxImageViewHandle> >,   // len = 128
uavs_slots: Vec<Option<GfxImageViewHandle> >,
```

**辅助（逆向查询）**：从 handle 反查 slot

```rust
srvs_handle_to_slot: SecondaryMap<GfxImageViewHandle, usize>,
uavs_handle_to_slot: SecondaryMap<GfxImageViewHandle, usize>,
```

**管理（slot 生命周期）**：

```rust
// 空闲 slot 池，register 时 pop 分配
srvs_free_slots: Vec<usize>,
uavs_free_slots: Vec<usize>,

// dirty 列表：key=slot，value=最后修改时的 frame_id
// 注册时 Some(handle)，注销后变 None，兼作 pending_reclaim
dirty_srvs: HashMap<usize, u64>,
dirty_uavs: HashMap<usize, u64>,
```

---

## 单套 Descriptor Set

`GlobalDescriptorSets` 中 `set_1_bindless` 只保留一个实例（区别于 `set_2_perframe` 的 FIF × 3）。

可以只维护一套 set 的前提：

1. **`UPDATE_UNUSED_WHILE_PENDING_BIT`**：允许 CPU 在有 in-flight 命令时更新 descriptor，只要被更新的 slot 不被这些命令动态访问。
2. **延迟 slot 回收**：`unregister` 后，旧 slot 进入 dirty 列表，等 age ≥ `FIF_COUNT` 后才归还 `free_slots`。此时所有引用旧
   slot 的 in-flight 命令已完成，新注册的资源写入该 slot 时不存在 GPU 并发读取。

---

## Slot 生命周期

### register

- register 时就可以为 ImageView 分配 slot，slot 分配后立即稳定，之后不变。
- 然后在 prepare_render_data 时写入 descriptor，之后不再更新。

### unregister

- unregister 时，slot 不能立即回收，因为 GPU 上可能还有 in-flight 命令访问这个 slot。只能先标记为 dirty，等 age ≥
  `FIF_COUNT` 后才归还 `free_slots`。

