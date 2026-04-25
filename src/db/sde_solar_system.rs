use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone)]
pub struct SdeSolarSystem {
    pub solar_system_id: i64,
    pub name: String,
    pub region_id: Option<i64>,
    pub constellation_id: Option<i64>,
    pub faction_id: Option<i64>,
    pub star_id: Option<i64>,
    pub security_status: Option<f32>,
    pub security_class: Option<String>,
    pub wh_class: Option<String>,
    pub wormhole_class_id: Option<i64>,
    pub luminosity: Option<f32>,
    pub radius: Option<f64>,
    pub border: Option<bool>,
    pub corridor: Option<bool>,
    pub fringe: Option<bool>,
    pub hub: Option<bool>,
    pub international: Option<bool>,
    pub regional: Option<bool>,
    pub visual_effect: Option<String>,
    pub name_i18n: Option<Value>,
    pub planet_ids: Option<Value>,
    pub stargate_ids: Option<Value>,
    pub disallowed_anchor_categories: Option<Value>,
    pub disallowed_anchor_groups: Option<Value>,
    pub position: Option<Value>,
    pub position_2d: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct SdeSolarSystemMetadata {
    pub id: i16,
    pub sde_version: String,
    pub sde_checksum: String,
    pub loaded_at: DateTime<Utc>,
}

pub async fn find_solar_system(pool: &PgPool, id: i64) -> Result<Option<SdeSolarSystem>> {
    sqlx::query_as!(
        SdeSolarSystem,
        r#"
        SELECT solar_system_id, name, region_id, constellation_id,
               faction_id, star_id,
               security_status, security_class, wh_class, wormhole_class_id,
               luminosity, radius,
               border, corridor, fringe, hub, international, regional,
               visual_effect,
               name_i18n as "name_i18n: Value",
               planet_ids as "planet_ids: Value",
               stargate_ids as "stargate_ids: Value",
               disallowed_anchor_categories as "disallowed_anchor_categories: Value",
               disallowed_anchor_groups as "disallowed_anchor_groups: Value",
               position as "position: Value",
               position_2d as "position_2d: Value"
        FROM sde_solar_system
        WHERE solar_system_id = $1
        "#,
        id,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch solar system")
}

pub async fn find_solar_systems_bulk(pool: &PgPool, ids: &[i64]) -> Result<Vec<SdeSolarSystem>> {
    sqlx::query_as!(
        SdeSolarSystem,
        r#"
        SELECT solar_system_id, name, region_id, constellation_id,
               faction_id, star_id,
               security_status, security_class, wh_class, wormhole_class_id,
               luminosity, radius,
               border, corridor, fringe, hub, international, regional,
               visual_effect,
               name_i18n as "name_i18n: Value",
               planet_ids as "planet_ids: Value",
               stargate_ids as "stargate_ids: Value",
               disallowed_anchor_categories as "disallowed_anchor_categories: Value",
               disallowed_anchor_groups as "disallowed_anchor_groups: Value",
               position as "position: Value",
               position_2d as "position_2d: Value"
        FROM sde_solar_system
        WHERE solar_system_id = ANY($1)
        "#,
        ids,
    )
    .fetch_all(pool)
    .await
    .context("failed to bulk fetch solar systems")
}

pub async fn bulk_upsert_solar_systems(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[SdeSolarSystem],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let ids: Vec<i64> = rows.iter().map(|r| r.solar_system_id).collect();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let region_ids: Vec<Option<i64>> = rows.iter().map(|r| r.region_id).collect();
    let constellation_ids: Vec<Option<i64>> = rows.iter().map(|r| r.constellation_id).collect();
    let faction_ids: Vec<Option<i64>> = rows.iter().map(|r| r.faction_id).collect();
    let star_ids: Vec<Option<i64>> = rows.iter().map(|r| r.star_id).collect();
    let security_statuses: Vec<Option<f32>> = rows.iter().map(|r| r.security_status).collect();
    let security_classes: Vec<Option<&str>> =
        rows.iter().map(|r| r.security_class.as_deref()).collect();
    let wh_classes: Vec<Option<&str>> = rows.iter().map(|r| r.wh_class.as_deref()).collect();
    let wormhole_class_ids: Vec<Option<i64>> = rows.iter().map(|r| r.wormhole_class_id).collect();
    let luminosities: Vec<Option<f32>> = rows.iter().map(|r| r.luminosity).collect();
    let radii: Vec<Option<f64>> = rows.iter().map(|r| r.radius).collect();
    let borders: Vec<Option<bool>> = rows.iter().map(|r| r.border).collect();
    let corridors: Vec<Option<bool>> = rows.iter().map(|r| r.corridor).collect();
    let fringes: Vec<Option<bool>> = rows.iter().map(|r| r.fringe).collect();
    let hubs: Vec<Option<bool>> = rows.iter().map(|r| r.hub).collect();
    let internationals: Vec<Option<bool>> = rows.iter().map(|r| r.international).collect();
    let regionals: Vec<Option<bool>> = rows.iter().map(|r| r.regional).collect();
    let visual_effects: Vec<Option<&str>> =
        rows.iter().map(|r| r.visual_effect.as_deref()).collect();
    let name_i18ns: Vec<Option<Value>> = rows.iter().map(|r| r.name_i18n.clone()).collect();
    let planet_ids_col: Vec<Option<Value>> = rows.iter().map(|r| r.planet_ids.clone()).collect();
    let stargate_ids_col: Vec<Option<Value>> =
        rows.iter().map(|r| r.stargate_ids.clone()).collect();
    let disallowed_anchor_categories_col: Vec<Option<Value>> = rows
        .iter()
        .map(|r| r.disallowed_anchor_categories.clone())
        .collect();
    let disallowed_anchor_groups_col: Vec<Option<Value>> = rows
        .iter()
        .map(|r| r.disallowed_anchor_groups.clone())
        .collect();
    let positions: Vec<Option<Value>> = rows.iter().map(|r| r.position.clone()).collect();
    let position_2ds: Vec<Option<Value>> = rows.iter().map(|r| r.position_2d.clone()).collect();

    let result = sqlx::query!(
        r#"
        INSERT INTO sde_solar_system (
            solar_system_id, name, region_id, constellation_id,
            faction_id, star_id,
            security_status, security_class, wh_class, wormhole_class_id,
            luminosity, radius,
            border, corridor, fringe, hub, international, regional,
            visual_effect,
            name_i18n, planet_ids, stargate_ids,
            disallowed_anchor_categories, disallowed_anchor_groups,
            position, position_2d
        )
        SELECT * FROM UNNEST(
            $1::bigint[],
            $2::text[],
            $3::bigint[],
            $4::bigint[],
            $5::bigint[],
            $6::bigint[],
            $7::real[],
            $8::text[],
            $9::text[],
            $10::bigint[],
            $11::real[],
            $12::double precision[],
            $13::boolean[],
            $14::boolean[],
            $15::boolean[],
            $16::boolean[],
            $17::boolean[],
            $18::boolean[],
            $19::text[],
            $20::jsonb[],
            $21::jsonb[],
            $22::jsonb[],
            $23::jsonb[],
            $24::jsonb[],
            $25::jsonb[],
            $26::jsonb[]
        )
        ON CONFLICT (solar_system_id) DO UPDATE
            SET name                          = EXCLUDED.name,
                region_id                     = EXCLUDED.region_id,
                constellation_id              = EXCLUDED.constellation_id,
                faction_id                    = EXCLUDED.faction_id,
                star_id                       = EXCLUDED.star_id,
                security_status               = EXCLUDED.security_status,
                security_class                = EXCLUDED.security_class,
                wh_class                      = COALESCE(EXCLUDED.wh_class, sde_solar_system.wh_class),
                wormhole_class_id             = EXCLUDED.wormhole_class_id,
                luminosity                    = EXCLUDED.luminosity,
                radius                        = EXCLUDED.radius,
                border                        = EXCLUDED.border,
                corridor                      = EXCLUDED.corridor,
                fringe                        = EXCLUDED.fringe,
                hub                           = EXCLUDED.hub,
                international                 = EXCLUDED.international,
                regional                      = EXCLUDED.regional,
                visual_effect                 = EXCLUDED.visual_effect,
                name_i18n                     = EXCLUDED.name_i18n,
                planet_ids                    = EXCLUDED.planet_ids,
                stargate_ids                  = EXCLUDED.stargate_ids,
                disallowed_anchor_categories  = EXCLUDED.disallowed_anchor_categories,
                disallowed_anchor_groups      = EXCLUDED.disallowed_anchor_groups,
                position                      = EXCLUDED.position,
                position_2d                   = EXCLUDED.position_2d
        "#,
        &ids,
        &names as &[&str],
        &region_ids as &[Option<i64>],
        &constellation_ids as &[Option<i64>],
        &faction_ids as &[Option<i64>],
        &star_ids as &[Option<i64>],
        &security_statuses as &[Option<f32>],
        &security_classes as &[Option<&str>],
        &wh_classes as &[Option<&str>],
        &wormhole_class_ids as &[Option<i64>],
        &luminosities as &[Option<f32>],
        &radii as &[Option<f64>],
        &borders as &[Option<bool>],
        &corridors as &[Option<bool>],
        &fringes as &[Option<bool>],
        &hubs as &[Option<bool>],
        &internationals as &[Option<bool>],
        &regionals as &[Option<bool>],
        &visual_effects as &[Option<&str>],
        &name_i18ns as &[Option<Value>],
        &planet_ids_col as &[Option<Value>],
        &stargate_ids_col as &[Option<Value>],
        &disallowed_anchor_categories_col as &[Option<Value>],
        &disallowed_anchor_groups_col as &[Option<Value>],
        &positions as &[Option<Value>],
        &position_2ds as &[Option<Value>],
    )
    .execute(&mut **tx)
    .await
    .context("failed to bulk upsert solar systems")?;

    Ok(result.rows_affected())
}

pub async fn current_sde_metadata(pool: &PgPool) -> Result<Option<SdeSolarSystemMetadata>> {
    sqlx::query_as!(
        SdeSolarSystemMetadata,
        r#"
        SELECT id, sde_version, sde_checksum, loaded_at
        FROM sde_solar_system_metadata
        WHERE id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .context("failed to fetch sde metadata")
}

pub async fn set_sde_metadata(
    tx: &mut Transaction<'_, Postgres>,
    version: &str,
    checksum: &str,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO sde_solar_system_metadata (id, sde_version, sde_checksum, loaded_at)
        VALUES (1, $1, $2, now())
        ON CONFLICT (id) DO UPDATE
            SET sde_version  = EXCLUDED.sde_version,
                sde_checksum = EXCLUDED.sde_checksum,
                loaded_at    = EXCLUDED.loaded_at
        "#,
        version,
        checksum,
    )
    .execute(&mut **tx)
    .await
    .context("failed to upsert sde metadata")?;
    Ok(())
}
