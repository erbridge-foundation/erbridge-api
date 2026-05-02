use utoipa::{
    Modify, OpenApi,
    openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme},
};

use crate::dto::health::HealthResponse;

#[derive(OpenApi)]
#[openapi(
    paths(),
    components(schemas(
        HealthResponse,
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "cookieAuth",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("erbridge_session"))),
        );
        components.add_security_scheme(
            "bearerAuth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some("API key prefixed with `erbridge_`"))
                    .build(),
            ),
        );
    }
}
