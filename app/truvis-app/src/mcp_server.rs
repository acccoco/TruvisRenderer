use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{
    CallToolResult, Implementation, ListResourceTemplatesResult, ListResourcesResult, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource, ResourceContents, ResourceTemplate,
    ServerCapabilities, ServerConfig,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use rmcp::{ErrorData as McpError, ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use truvis_editor_bridge::protocol::{
    AutomationCommand, AutomationQuery, AutomationRequest, AutomationResponse, InstanceId, InstanceTransformDto,
    MaterialId, MaterialPatch, SceneVersion,
};
use truvis_editor_bridge::{AutomationFrontendEndpoint, RequestEnvelope};

pub(crate) struct McpServerHandle {
    shutdown: Option<CancellationToken>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl McpServerHandle {
    pub(crate) fn start(endpoint: AutomationFrontendEndpoint) -> Result<Self, String> {
        let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
        let cancellation = CancellationToken::new();
        let server_cancellation = cancellation.clone();
        let thread = std::thread::Builder::new()
            .name("truvis-mcp-http".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
                let _ = runtime.block_on(McpServer::run(endpoint, server_cancellation, ready_sender));
            })
            .map_err(|error| error.to_string())?;
        match ready_receiver.recv().map_err(|_| "MCP server thread stopped during startup".to_string())? {
            Ok(()) => Ok(Self {
                shutdown: Some(cancellation),
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        self.stop_accepting();
        self.join();
    }

    pub(crate) fn stop_accepting(&self) {
        if let Some(token) = self.shutdown.as_ref() {
            token.cancel();
        }
    }

    pub(crate) fn join(&mut self) {
        let _ = self.shutdown.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for McpServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Clone)]
struct AutomationClient {
    sender: mpsc::Sender<RequestEnvelope<AutomationRequest, AutomationResponse>>,
}

impl AutomationClient {
    async fn request(&self, request: AutomationRequest) -> AutomationResponse {
        let (reply_sender, reply_receiver) = oneshot::channel();
        if let Err(error) = self.sender.try_send(RequestEnvelope {
            request,
            reply: reply_sender,
        }) {
            let (code, message) = match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    (truvis_editor_bridge::protocol::EditorErrorCode::Busy, "automation queue is full")
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    (truvis_editor_bridge::protocol::EditorErrorCode::Shutdown, "RenderThread is shutting down")
                }
            };
            return AutomationResponse::Error(truvis_editor_bridge::protocol::EditorError::new(code, message));
        }
        reply_receiver.await.unwrap_or_else(|_| {
            AutomationResponse::Error(truvis_editor_bridge::protocol::EditorError::new(
                truvis_editor_bridge::protocol::EditorErrorCode::Shutdown,
                "RenderThread is shutting down",
            ))
        })
    }
}

struct McpServer {
    client: AutomationClient,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SceneObjectsArgs {
    pub offset: Option<u32>,
    pub limit: Option<u16>,
    pub expected_scene_version: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InstanceArgs {
    pub instance_id: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MaterialArgs {
    pub material_id: String,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TransformArgs {
    pub instance_id: String,
    pub location: [f32; 3],
    pub rotation_degrees: [f32; 3],
    pub scale: [f32; 3],
    pub expected_scene_version: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MaterialUpdateArgs {
    pub material_id: String,
    pub patch: MaterialPatchArgs,
    pub expected_scene_version: Option<String>,
}
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
struct MaterialPatchArgs {
    pub name: Option<String>,
    pub base_color: Option<[f32; 4]>,
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub class: Option<MaterialClassArg>,
    pub coverage: Option<CoverageArg>,
    pub texture_mappings: Option<Vec<TextureMappingPatchArg>>,
    pub normal_scale: Option<f32>,
    pub emissive_factor: Option<[f32; 3]>,
}
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MaterialClassArg {
    Surface,
    Transmission { opacity: f32, ior: f32 },
    Emissive { radiance: [f32; 3] },
}
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CoverageArg {
    Opaque,
    AlphaMask { alpha_cutoff: f32 },
}
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
struct TextureMappingPatchArg {
    pub channel: u32,
    pub mapping: TextureMappingArg,
}
#[derive(Debug, Deserialize, serde::Serialize, schemars::JsonSchema)]
struct TextureMappingArg {
    pub tex_coord: u32,
    pub offset: [f32; 2],
    pub rotation: f32,
    pub scale: [f32; 2],
    pub wrap: [u32; 2],
    pub filter: u32,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ImportArgs {
    pub path: String,
    pub expected_scene_version: Option<String>,
}
#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ImportStatusArgs {
    pub import_id: String,
}

impl McpServer {
    const LISTEN_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8765);
    const PATH: &'static str = "/mcp";

    fn new(client: AutomationClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    async fn run(
        endpoint: AutomationFrontendEndpoint,
        cancellation: CancellationToken,
        ready_sender: std::sync::mpsc::SyncSender<Result<(), String>>,
    ) -> Result<(), std::io::Error> {
        let (sender, _notifications) = endpoint.into_parts();
        let handler = Arc::new(McpServer::new(AutomationClient { sender }));
        let service = StreamableHttpService::new(
            {
                let handler = Arc::clone(&handler);
                move || Ok((*handler).clone())
            },
            Arc::new(NeverSessionManager::default()),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(false)
                .with_json_response(true)
                .with_stateless_protocol_metadata_required(false)
                .with_allowed_hosts(["127.0.0.1:8765"])
                // Axum 的 graceful shutdown 负责停止新连接；不取消 RMCP 内部 token，
                // 让已经进入 handler 的请求继续等待 RenderThread reply。
                .with_cancellation_token(CancellationToken::new()),
        );
        let router = Router::new().route(Self::PATH, axum::routing::any_service(service));
        let listener = match TcpListener::bind(Self::LISTEN_ADDRESS).await {
            Ok(listener) => listener,
            Err(error) => {
                let message = format!(
                    "failed to bind Truvis MCP at {}: {}. Release the port and restart Truvis.",
                    Self::LISTEN_ADDRESS,
                    error,
                );
                log::error!("{message}");
                let _ = ready_sender.send(Err(message.clone()));
                return Err(std::io::Error::new(error.kind(), message));
            }
        };
        let _ = ready_sender.send(Ok(()));
        log::info!("Truvis MCP Streamable HTTP listening on http://{}{}", Self::LISTEN_ADDRESS, Self::PATH);
        axum::serve(listener, router)
            .with_graceful_shutdown(cancellation.cancelled_owned())
            .await
            .map_err(std::io::Error::other)
    }

    async fn result(&self, response: AutomationResponse) -> Result<CallToolResult, McpError> {
        let error = matches!(response, AutomationResponse::Error(_));
        let value =
            serde_json::to_value(response).map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(if error { CallToolResult::structured_error(value) } else { CallToolResult::structured(value) })
    }

    fn version(value: Option<String>) -> Result<Option<SceneVersion>, McpError> {
        Ok(value.map(SceneVersion))
    }
}

#[tool_router]
impl McpServer {
    #[tool(
        name = "truvis_get_scene_summary",
        description = "Return the current CPU scene summary and scene version"
    )]
    async fn get_scene_summary(&self) -> Result<CallToolResult, McpError> {
        self.result(self.client.request(AutomationRequest::Query(AutomationQuery::GetSceneSummary)).await).await
    }
    #[tool(
        name = "truvis_list_scene_objects",
        description = "List a bounded page of CPU scene objects"
    )]
    async fn list_scene_objects(
        &self,
        Parameters(args): Parameters<SceneObjectsArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Query(AutomationQuery::GetSceneObjects {
                    offset: args.offset.unwrap_or(0),
                    limit: args.limit.unwrap_or(128),
                    expected_scene_version: Self::version(args.expected_scene_version)?,
                }))
                .await,
        )
        .await
    }
    #[tool(name = "truvis_get_instance_details", description = "Read one scene instance")]
    async fn get_instance_details(
        &self,
        Parameters(args): Parameters<InstanceArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Query(AutomationQuery::GetInstanceDetails {
                    instance_id: InstanceId::new(args.instance_id),
                }))
                .await,
        )
        .await
    }
    #[tool(name = "truvis_get_material", description = "Read one material")]
    async fn get_material(&self, Parameters(args): Parameters<MaterialArgs>) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Query(AutomationQuery::GetMaterial {
                    material_id: MaterialId::new(args.material_id),
                }))
                .await,
        )
        .await
    }
    #[tool(
        name = "truvis_update_transform",
        description = "Update one instance transform in the CPU scene"
    )]
    async fn update_transform(&self, Parameters(args): Parameters<TransformArgs>) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Command(AutomationCommand::UpdateTransform {
                    instance_id: InstanceId::new(args.instance_id),
                    transform: InstanceTransformDto {
                        location: args.location,
                        rotation_degrees: args.rotation_degrees,
                        scale: args.scale,
                    },
                    expected_scene_version: Self::version(args.expected_scene_version)?,
                }))
                .await,
        )
        .await
    }
    #[tool(
        name = "truvis_update_material",
        description = "Apply a validated patch to one material"
    )]
    async fn update_material(
        &self,
        Parameters(args): Parameters<MaterialUpdateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let patch: MaterialPatch = serde_json::from_value(
            serde_json::to_value(args.patch)
                .map_err(|error| McpError::invalid_params(format!("invalid material patch: {error}"), None))?,
        )
        .map_err(|error| McpError::invalid_params(format!("invalid material patch: {error}"), None))?;
        self.result(
            self.client
                .request(AutomationRequest::Command(AutomationCommand::UpdateMaterial {
                    material_id: MaterialId::new(args.material_id),
                    patch,
                    expected_scene_version: Self::version(args.expected_scene_version)?,
                }))
                .await,
        )
        .await
    }
    #[tool(
        name = "truvis_import_scene",
        description = "Start a CPU scene import and return its opaque import ID"
    )]
    async fn import_scene(&self, Parameters(args): Parameters<ImportArgs>) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Command(AutomationCommand::ImportScene {
                    path: args.path,
                    expected_scene_version: Self::version(args.expected_scene_version)?,
                }))
                .await,
        )
        .await
    }
    #[tool(name = "truvis_get_scene_import_status", description = "Read a scene import status")]
    async fn get_scene_import_status(
        &self,
        Parameters(args): Parameters<ImportStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.result(
            self.client
                .request(AutomationRequest::Query(AutomationQuery::GetSceneImportStatus {
                    import_id: args.import_id,
                }))
                .await,
        )
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new("truvis", env!("CARGO_PKG_VERSION")))
            .with_instructions("Truvis CPU scene automation")
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2026_07_28,
        ])
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![Resource::new("truvis://scene/summary", "scene_summary")]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("truvis://scene/objects/{offset}", "scene_objects"),
            ResourceTemplate::new("truvis://scene/instances/{id}", "scene_instance"),
            ResourceTemplate::new("truvis://scene/materials/{id}", "scene_material"),
            ResourceTemplate::new("truvis://scene/imports/{id}", "scene_import"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let query = if request.uri == "truvis://scene/summary" {
            AutomationQuery::GetSceneSummary
        } else if let Some(offset) =
            request.uri.strip_prefix("truvis://scene/objects/").and_then(|value| value.parse().ok())
        {
            AutomationQuery::GetSceneObjects {
                offset,
                limit: 128,
                expected_scene_version: None,
            }
        } else if let Some(id) = request.uri.strip_prefix("truvis://scene/instances/") {
            AutomationQuery::GetInstanceDetails {
                instance_id: InstanceId::new(id),
            }
        } else if let Some(id) = request.uri.strip_prefix("truvis://scene/materials/") {
            AutomationQuery::GetMaterial {
                material_id: MaterialId::new(id),
            }
        } else if let Some(id) = request.uri.strip_prefix("truvis://scene/imports/") {
            AutomationQuery::GetSceneImportStatus {
                import_id: id.to_string(),
            }
        } else {
            return Err(McpError::resource_not_found(request.uri, None));
        };
        let response = self.client.request(AutomationRequest::Query(query)).await;
        let value =
            serde_json::to_string(&response).map_err(|error| McpError::internal_error(error.to_string(), None))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(value, request.uri)]).into())
    }
}

impl Clone for McpServer {
    fn clone(&self) -> Self {
        Self::new(self.client.clone())
    }
}
