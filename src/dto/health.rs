use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: ComponentState,
    pub version: String,
    pub components: Components,
}

#[derive(Serialize, ToSchema)]
pub struct Components {
    pub database: ComponentState,
}

#[derive(Serialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ComponentState {
    Ok,
    Degraded,
}
