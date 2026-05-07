# gpu-resource-lifecycle Specification

## Purpose
TBD - created by archiving change unify-gpu-resource-lifecycle. Update Purpose after archive.
## Requirements
### Requirement: GPU resource wrappers SHALL declare one lifecycle contract

Every GPU-owning Rust wrapper SHALL follow exactly one lifecycle contract: RAII-owned or manager-owned. A RAII-owned wrapper SHALL release its Vulkan/VMA object in `Drop`. A manager-owned wrapper SHALL release its Vulkan/VMA object only through its lifecycle owner or manager, and its `Drop` SHALL NOT perform raw Vulkan/VMA destruction directly.

#### Scenario: RAII-owned resource is dropped

- **WHEN** a RAII-owned GPU wrapper leaves scope before `Gfx::destroy()`
- **THEN** its `Drop` implementation SHALL release its owned Vulkan/VMA object exactly once
- **AND** any public `destroy(self)` method SHALL be only a documented drop-now alias
- **AND** the wrapper SHALL NOT expose `destroy_mut(&mut self)`

#### Scenario: Manager-owned resource is dropped without explicit release

- **WHEN** a manager-owned GPU wrapper is dropped directly without manager release
- **THEN** `Drop` SHALL NOT call Vulkan/VMA destruction
- **AND** debug builds SHALL surface the lifecycle violation through an assertion or diagnostic log

### Requirement: Resource managers SHALL own delayed and dependency-ordered destruction

Resource managers SHALL own destruction that depends on frame completion, bindless unregistering, or parent-child ordering such as image-view-before-image. Callers SHALL request release through manager APIs rather than destroying manager-owned resource values directly.

#### Scenario: Image is released through the resource manager

- **WHEN** a managed image is released through delayed cleanup or immediate release
- **THEN** the resource manager SHALL destroy all image views associated with that image before destroying the image
- **AND** pending destroy queues SHALL be updated so the same handle is not destroyed again

#### Scenario: Frame-delayed buffer or image is cleaned

- **WHEN** a delayed release reaches the frame-safe cleanup point
- **THEN** the resource manager SHALL remove the resource from its pool and release it once
- **AND** the release path SHALL include the current destroy reason in diagnostics

### Requirement: Pending GPU uploads SHALL be drained before teardown

Upload managers that hold GPU resources for in-flight transfers SHALL provide an explicit shutdown path. Shutdown SHALL wait for, drain, or explicitly cancel pending uploads before command pools, semaphores, resource managers, allocators, or `Gfx` are destroyed.

#### Scenario: Asset upload manager shuts down with pending uploads

- **WHEN** `AssetUploadManager` shutdown starts while pending uploads still hold staging buffers, command buffers, or destination images
- **THEN** the upload manager SHALL release those resources while `Gfx` is still alive
- **AND** command buffers SHALL be freed or made unreachable before their command pool is destroyed
- **AND** pending destination images SHALL either be transferred to their manager owner or explicitly destroyed
- **AND** the timeline semaphore and command pool SHALL be destroyed exactly once

### Requirement: VMA allocation creation SHALL include project debug user data

VMA-backed resource creation SHALL attach stable project debug names to allocations using `vk_mem::AllocationCreateInfo::user_data` together with `vk_mem::AllocationCreateFlags::USER_DATA_COPY_STRING`. The raw pointer handling for this metadata SHALL be encapsulated in a project helper instead of repeated at allocation call sites.

#### Scenario: Buffer allocation is created

- **WHEN** `GfxBuffer::new` creates a VMA buffer allocation
- **THEN** the allocation create info SHALL include copied VMA user data derived from the buffer debug name
- **AND** callers SHALL NOT need to provide raw VMA user data pointers

#### Scenario: Image allocation is created

- **WHEN** `GfxImage::new` creates a VMA image allocation
- **THEN** the allocation create info SHALL include copied VMA user data derived from the image debug name
- **AND** externally owned images SHALL NOT pretend to own VMA allocation metadata

#### Scenario: Helper handles user data pointer lifetime

- **WHEN** project code needs VMA debug user data for an allocation
- **THEN** it SHALL call the project helper
- **AND** the helper SHALL keep any temporary string storage alive for the duration of the VMA create call
- **AND** VMA SHALL own the copied string after creation

### Requirement: Destruction diagnostics SHALL identify resources

GPU destruction paths SHALL provide enough diagnostics to correlate Vulkan/VMA errors with project resources. Diagnostics SHALL include the project debug name when available, raw Vulkan handle, manager handle when available, and a typed destroy reason.

#### Scenario: Resource is destroyed during shutdown

- **WHEN** a GPU resource is destroyed as part of shutdown
- **THEN** diagnostics SHALL identify the resource name and shutdown destroy reason

#### Scenario: Resource is destroyed due to resize or immediate release

- **WHEN** a GPU resource is destroyed because of resize or immediate release
- **THEN** diagnostics SHALL distinguish that reason from normal shutdown and deferred cleanup

#### Scenario: Manager release path reports destroy reason

- **WHEN** a resource manager releases a resource through delayed cleanup, immediate release, resize, or shutdown
- **THEN** the release API SHALL pass or derive the corresponding destroy reason
- **AND** the concrete destruction log SHALL include that reason

### Requirement: Cloneable handle wrappers SHALL NOT be implicit owners

Wrappers that can be cloned SHALL NOT act as independent owners of a single raw Vulkan handle unless they use an explicit shared ownership model with exactly-once destruction. Cloneable non-owning values SHALL expose raw handles or lightweight references without a public `destroy(self)` owner API.

#### Scenario: Semaphore handle is shared with submit code

- **WHEN** queue submit or render graph code needs a semaphore handle
- **THEN** it SHALL use a reference or raw handle accessor from the owning semaphore
- **AND** cloning the accessor SHALL NOT create another owner that can destroy the semaphore

#### Scenario: Owned semaphore is destroyed

- **WHEN** an owned semaphore reaches its shutdown point
- **THEN** exactly one owner SHALL destroy the raw Vulkan semaphore
- **AND** no cloned wrapper value SHALL be able to destroy the same handle again

