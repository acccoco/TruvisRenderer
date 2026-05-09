## ADDED Requirements

### Requirement: RenderGraph uses pass addition order
RenderGraph SHALL execute passes in the exact order they are added by the App or contributing Plugin. RenderGraph SHALL NOT reorder passes with dependency graph analysis or topological sorting.

#### Scenario: Passes execute in insertion order
- **WHEN** an App adds pass A, then pass B, then pass C to a RenderGraph
- **THEN** RenderGraph records pass A before pass B before pass C

#### Scenario: Resource dependencies do not reorder passes
- **WHEN** two passes declare image reads and writes that could form a DAG dependency
- **THEN** RenderGraph preserves the original add order and uses the declarations only for synchronization and validation

### Requirement: Resource access declarations drive synchronization
RenderGraph SHALL use each pass's declared image reads and writes to compute the image barriers required before that pass in the fixed execution order.

#### Scenario: Write then read inserts barrier
- **WHEN** pass A writes an imported image and pass B later reads that image
- **THEN** RenderGraph inserts a barrier before pass B from A's write state to B's read state

#### Scenario: Read then write inserts barrier
- **WHEN** pass A reads an imported image and pass B later writes that image
- **THEN** RenderGraph inserts a barrier before pass B that waits for A's read state before entering B's write state

#### Scenario: Layout transition inserts barrier
- **WHEN** a later pass requires a different image layout from the tracked current image layout
- **THEN** RenderGraph inserts a layout transition barrier before that pass

### Requirement: Sequential state tracking covers read and write states
RenderGraph SHALL track image state linearly across the fixed execution order. Consecutive read-only accesses to the same image MAY be merged for tracking, but a later write MUST wait for the tracked read states.

#### Scenario: Consecutive reads remain read-only
- **WHEN** multiple consecutive passes read the same image without layout changes or intervening writes
- **THEN** RenderGraph avoids unnecessary write-style barriers between those read-only passes

#### Scenario: Write after multiple reads waits for reads
- **WHEN** one or more passes read an image and a later pass writes that same image
- **THEN** RenderGraph computes the write barrier from the tracked read state rather than ignoring the prior reads

### Requirement: Public API reflects implemented capability
RenderGraph SHALL expose only implemented graph capabilities as public API. Unsupported transient graph resources, buffer graph access, dependency graph internals, and incomplete barrier paths SHALL NOT be part of the public RenderGraph contract.

#### Scenario: Imported image workflow remains public
- **WHEN** an App needs to record passes against swapchain, FIF, or persistent render target images
- **THEN** RenderGraph provides public APIs to import images, declare image access, execute pass recording, and collect external semaphore submit data

#### Scenario: Unsupported resource models stay private
- **WHEN** transient image allocation, transient buffer allocation, or buffer barrier recording is not fully implemented
- **THEN** RenderGraph does not advertise those capabilities through public APIs or current documentation

### Requirement: RenderGraph preserves App ownership boundaries
RenderGraph SHALL remain a GPU resource synchronization and command recording helper. App and concrete Plugin code SHALL continue to own render pass composition, command buffer lifetime, queue submit timing, and high-level render pipeline order.

#### Scenario: App controls graph composition
- **WHEN** an App combines render pipeline passes, resolve passes, and GUI overlay passes
- **THEN** the App determines their add order and RenderGraph records them in that order

#### Scenario: RenderGraph does not own submission policy
- **WHEN** RenderGraph finishes command recording
- **THEN** App or backend code remains responsible for ending command buffers and submitting them to the chosen queue
