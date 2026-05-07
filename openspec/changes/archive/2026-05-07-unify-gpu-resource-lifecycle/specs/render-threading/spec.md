## ADDED Requirements

### Requirement: App and plugin GPU resources SHALL be released before Gfx teardown

During render-thread shutdown, app-owned and plugin-owned GPU resources SHALL be released on the render thread before `RenderBackend` is destroyed and before `Gfx::destroy()` is called. After `Gfx::destroy()` begins, no remaining app/plugin resource `Drop` implementation may call `Gfx::get()` or any Vulkan/VMA destruction API through the project wrappers.

#### Scenario: Render thread shuts down app-owned plugins

- **WHEN** the render loop observes the exit flag and calls `RenderApp::shutdown`
- **THEN** the app hooks and standard plugin shutdown traversal SHALL receive typed shutdown context where manager-owned resources need backend manager access
- **AND** they SHALL release all app/plugin-owned GPU resources while `Gfx` and `RenderBackend` are still alive
- **AND** `RenderBackend::destroy()` SHALL run only after that release phase
- **AND** `Gfx::destroy()` SHALL run only after `RenderBackend::destroy()` completes

#### Scenario: Plugin releases manager-owned resources

- **WHEN** a plugin owns bindless registrations or handles to resources stored in `GfxResourceManager`
- **THEN** `Plugin::shutdown` or the equivalent typed shutdown traversal SHALL expose the required `RenderWorld` manager access
- **AND** the plugin SHALL unregister bindless references before destroying the associated manager-owned images or views
- **AND** the manager SHALL perform image-view-before-image destruction ordering

#### Scenario: App value drops after Gfx teardown

- **WHEN** the concrete app value is later dropped by Rust after `RenderApp::shutdown` has returned
- **THEN** no remaining app/plugin field drop SHALL require `Gfx::get()`
- **AND** debug builds SHALL surface a lifecycle violation if an app/plugin GPU owner was not released during shutdown
