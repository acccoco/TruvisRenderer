## 1. Current Usage Audit

- [x] 1.1 Audit all workspace imports of `truvis_render_graph::render_graph::*` and list APIs that are actually used.
- [x] 1.2 Confirm existing App render hooks only rely on imported image workflows and external semaphore submit data.
- [x] 1.3 Confirm no production caller uses RenderGraph transient image/buffer or buffer read/write APIs.

## 2. Sequential Execution Model

- [x] 2.1 Remove topology sorting from the execution path and make pass insertion order the stored execution order.
- [x] 2.2 Remove `DependencyGraph` / `EdgeData` from public API and delete the `petgraph` dependency if no longer used.
- [x] 2.3 Keep existing pass registration call sites compiling while replacing internal dependency analysis with linear order tracking.

## 3. Barrier State Tracking

- [x] 3.1 Implement a linear image state tracker initialized from imported image states.
- [x] 3.2 Generate image barriers for write-after-read, read-after-write, write-after-write, and layout transition cases.
- [x] 3.3 Track consecutive read-only states so a later write waits for prior reads without adding unnecessary read-to-read barriers.
- [x] 3.4 Add focused unit tests for insertion order, read-after-write, write-after-read, write-after-write, layout transitions, and consecutive reads.

## 4. Public API Cleanup

- [x] 4.1 Hide or remove unsupported transient image/buffer creation APIs from the public RenderGraph contract.
- [x] 4.2 Hide or remove public buffer access/state/barrier APIs until buffer barrier recording is implemented.
- [x] 4.3 Hide internal resource manager, resource node, barrier desc, and dependency graph implementation details from re-exports.
- [x] 4.4 Keep imported image, image state, image handle, pass context, pass registration, execution, semaphore, and execution-plan APIs available to current callers.

## 5. Call Site Migration

- [x] 5.1 Update `truvis-app` render hooks and plugin pass contribution code for any RenderGraph API naming or import changes.
- [x] 5.2 Update `truvis-render-passes` pass adapters for any pass context or access declaration changes.
- [x] 5.3 Verify RT compute graph, RT present graph, Triangle, ShaderToy, and GUI overlay paths still express their intended order through pass addition order.

## 6. Documentation And Verification

- [x] 6.1 Update `engine/crates/truvis-render-graph/README.md` to describe sequential execution instead of topological execution.
- [x] 6.2 Update `ARCHITECTURE.md` to remove current transient RenderGraph resource wording or mark it as future work.
- [x] 6.3 Run the repository Rust formatting command.
- [x] 6.4 Run the repository Rust check/build command from `justfile`.
- [x] 6.5 Run at least one lightweight demo smoke test when local Vulkan/runtime environment permits.
- [x] 6.6 Run `openspec validate simplify-render-graph --strict`.
