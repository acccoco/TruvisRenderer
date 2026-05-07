# gfx-explicit-context-lifecycle Specification

## Purpose
TBD - created by archiving change remove-global-gfx-context. Update Purpose after archive.
## Requirements
### Requirement: Gfx is an explicit root owner

`truvis-gfx` SHALL expose `Gfx` as an explicit Rust-owned root object rather than a process-wide singleton. The implementation SHALL NOT contain `G_GFX`, `static mut` global Gfx storage, or public `Gfx::get()` / `Gfx::init()` / `Gfx::destroy()` singleton APIs after migration.

#### Scenario: Gfx construction is explicit
- **WHEN** the render thread creates the render backend
- **THEN** `Gfx` SHALL be constructed through an owned value such as `Gfx::new(...)`
- **AND** the returned owner SHALL be stored in the render-thread-owned backend lifecycle

#### Scenario: No global Gfx access remains
- **WHEN** source code under `engine/crates` is searched for `Gfx::get`, `Gfx::init`, `Gfx::destroy`, or `G_GFX`
- **THEN** no production Rust code SHALL contain those singleton access patterns

### Requirement: Typed Gfx Ctx exposes only required capabilities

`Gfx` SHALL provide typed borrowed context views for common capability groups. Each API that needs Vulkan context SHALL accept the narrowest context that covers its operation rather than an unrestricted `&Gfx`.

#### Scenario: Resource allocation receives resource context
- **WHEN** a buffer or allocated image is created or destroyed
- **THEN** the API SHALL receive a resource context containing device and allocator access
- **AND** the signature SHALL NOT require a full `&Gfx` when device and allocator are sufficient

#### Scenario: Descriptor and pipeline operations receive device context
- **WHEN** descriptor layouts, descriptor pools, shader modules, pipeline layouts, pipelines, samplers, or query pools are created or destroyed
- **THEN** the API SHALL receive a device context
- **AND** the signature SHALL NOT expose allocator or queue capabilities unless used

#### Scenario: Queue submission receives queue context or queue reference
- **WHEN** command batches are submitted or queue labels are recorded
- **THEN** the API SHALL receive an explicit queue context or explicit command queue reference

#### Scenario: Immediate command helpers receive immediate context
- **WHEN** a helper records, submits, waits for, and frees a temporary one-time command buffer
- **THEN** the API SHALL receive an explicit immediate-command context containing only the command pool, device, and queue capabilities it uses
- **AND** it SHALL NOT call a global Gfx accessor internally

#### Scenario: Device information queries receive information context
- **WHEN** code queries queue families, supported formats, device limits, or ray tracing pipeline properties
- **THEN** the API SHALL receive an explicit device-information context
- **AND** the signature SHALL NOT require a full `&Gfx` when read-only instance/physical-device properties are sufficient

#### Scenario: Debug naming and labels receive explicit context
- **WHEN** objects receive debug names or command/queue labels are recorded
- **THEN** the API SHALL receive explicit device, command, or queue debug capability
- **AND** it SHALL NOT fetch debug utilities through a global Gfx accessor

#### Scenario: Surface and swapchain operations receive surface context
- **WHEN** a surface or swapchain is created, queried, acquired, presented, rebuilt, or destroyed
- **THEN** the API SHALL receive explicit context for the Vulkan entry, instance, physical device, device, and queue capabilities it uses

### Requirement: Vulkan resources are explicitly destroyed

Vulkan/VMA/WSI handle wrappers SHALL release Vulkan/VMA/WSI resources only through explicit destroy APIs that receive the required typed Gfx Ctx. Destruction SHALL be performed by the owner responsible for that resource's lifecycle phase.

#### Scenario: Leaf resource destroy consumes the resource
- **WHEN** a leaf resource can be removed from its owner without leaving a placeholder
- **THEN** it SHOULD provide `destroy(self, ctx, reason)` or an equivalent consuming API
- **AND** the resource SHALL NOT be usable after the call due to Rust move semantics

#### Scenario: Managed resource destroy mutates in place
- **WHEN** a resource is managed inside resize paths, arrays, delayed release queues, or long-lived manager slots
- **THEN** it MAY provide `destroy_mut(&mut self, ctx, reason)` or an equivalent in-place API
- **AND** the implementation SHALL mark its handle/allocation as destroyed or null before returning

#### Scenario: Destroy path does not use global context
- **WHEN** any `destroy` or `destroy_mut` implementation releases a Vulkan/VMA/WSI object
- **THEN** it SHALL use the typed Gfx Ctx provided by the caller
- **AND** it SHALL NOT call a global Gfx accessor

#### Scenario: Surface and swapchain are explicit shutdown resources
- **WHEN** a surface or swapchain wrapper is destroyed during resize or backend shutdown
- **THEN** it SHALL release the WSI handle through an explicit destroy API
- **AND** its `Drop` implementation SHALL NOT call `destroy_surface`, `destroy_swapchain`, or equivalent Vulkan release APIs

### Requirement: Drop is diagnostic only for Vulkan resources

`Drop` implementations for Vulkan/VMA/WSI handle wrappers SHALL NOT call Vulkan/VMA/WSI release APIs. They SHALL only validate that explicit destruction already happened, with debug assertions or equivalent diagnostics.

#### Scenario: Explicitly destroyed resource drops cleanly
- **WHEN** a resource was explicitly destroyed before leaving scope
- **THEN** its `Drop` implementation SHALL perform no Vulkan/VMA/WSI release work
- **AND** debug assertions SHALL pass

#### Scenario: Undestroyed resource reaches Drop in debug
- **WHEN** a resource still owns a valid Vulkan/VMA handle during `Drop` in a debug build
- **THEN** the implementation SHALL report the missing explicit destroy through a debug assertion or equivalent diagnostic

### Requirement: Long-lived GPU resources do not store Gfx Ctx

Long-lived GPU resource wrappers outside the `Gfx` root owner SHALL NOT store typed Gfx Ctx, `&Gfx`, `&GfxDevice`, or `&VMemAllocator` references for later destruction. They SHALL store only their owned Vulkan/VMA handles, allocation metadata, debug name, and explicit destroyed/null state.

#### Scenario: RenderWorld resources are lifetime-free
- **WHEN** `RenderWorld`, `GfxResourceManager`, Plugin fields, or other long-lived owners store GPU resource wrappers
- **THEN** those owners SHALL NOT gain lifetime parameters solely because resources retained Gfx Ctx references

#### Scenario: Destruction dependencies are passed by owner
- **WHEN** an owner releases a stored GPU resource
- **THEN** the owner SHALL pass the required typed Gfx Ctx at the destruction call site

### Requirement: Gfx root owner is destroyed after all child GPU resources

The `Gfx` root owner SHALL outlive every Vulkan/VMA/WSI child resource created from its contexts. Backend and plugin shutdown SHALL explicitly release child resources before destroying `Gfx`.

#### Scenario: Backend shutdown releases children first
- **WHEN** the render backend is destroyed
- **THEN** app/plugin-owned GPU resources, present surface/swapchain resources, render-world resources, managers, command allocators, synchronization objects, and descriptor/pipeline resources SHALL be explicitly destroyed before `Gfx` root owner destruction

#### Scenario: Gfx destroy has no hidden child cleanup dependency
- **WHEN** `Gfx` root owner destruction begins
- **THEN** no remaining non-root Vulkan wrapper SHALL need to call Vulkan/VMA/WSI release APIs from `Drop`

