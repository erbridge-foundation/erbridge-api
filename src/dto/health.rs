use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: ComponentState,
    pub version: String,
    pub components: Components,
}

#[derive(Serialize)]
pub struct Components {
    pub database: ComponentState,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentState {
    Ok,
    Degraded,
}
