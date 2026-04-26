use anyhow::{Context, Result};
use sqlx::PgPool;
use uuid::Uuid;

pub struct RouteRow {
    pub current_system_id: i64,
    pub path_systems: Vec<i64>,
    pub path_connections: Vec<Uuid>,
    pub depth: i32,
}

pub async fn find_routes(
    pool: &PgPool,
    map_id: Uuid,
    start_system_id: i64,
    max_depth: i32,
    exclude_eol: bool,
    exclude_mass_critical: bool,
) -> Result<Vec<RouteRow>> {
    sqlx::query_as!(
        RouteRow,
        r#"
        WITH RECURSIVE routes AS (
            SELECT
                e.to_system_id                              AS current_system_id,
                ARRAY[e.from_system_id, e.to_system_id]    AS path_systems,
                ARRAY[e.connection_id]                      AS path_connections,
                1                                           AS depth
            FROM system_edges e
            WHERE e.map_id = $1
              AND e.from_system_id = $2
              AND e.status NOT IN ('collapsed', 'expired')
              AND (NOT $3 OR e.mass_state IS DISTINCT FROM 'critical')
              AND (NOT $4 OR e.life_state IS DISTINCT FROM 'eol')

            UNION ALL

            SELECT
                e.to_system_id,
                r.path_systems    || e.to_system_id,
                r.path_connections || e.connection_id,
                r.depth + 1
            FROM routes r
            JOIN system_edges e
              ON e.map_id = $1
             AND e.from_system_id = r.current_system_id
            WHERE r.depth < $5
              AND e.status NOT IN ('collapsed', 'expired')
              AND (NOT $3 OR e.mass_state IS DISTINCT FROM 'critical')
              AND (NOT $4 OR e.life_state IS DISTINCT FROM 'eol')
              AND NOT (e.to_system_id = ANY(r.path_systems))
        )
        SELECT
            current_system_id AS "current_system_id!: i64",
            path_systems      AS "path_systems!: Vec<i64>",
            path_connections  AS "path_connections!: Vec<Uuid>",
            depth             AS "depth!: i32"
        FROM routes
        ORDER BY depth ASC, current_system_id ASC
        "#,
        map_id,
        start_system_id,
        exclude_mass_critical,
        exclude_eol,
        max_depth,
    )
    .fetch_all(pool)
    .await
    .context("failed to find routes")
}
