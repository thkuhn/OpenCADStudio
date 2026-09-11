//! AEC commands — wall/room/storey creation, room schedule, IFC export.
//!
//! `AEC_WALL` is an interactive chain of 2-point wall segments (still
//! analogous to `PLINE` for clicks); the rest remain non-interactive scaffold
//! commands (matching the former plugin's pragmatic behaviour) that operate
//! directly on `&mut Scene` / the document and report feedback via the
//! command line.

use std::sync::Mutex;
use uuid::Uuid;

use acadrust::entities::{LwPolyline, LwVertex, Point, Table};
use acadrust::tables::AppId;
use acadrust::types::{Vector2, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};
use glam::{DVec2, DVec3};

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;
use crate::ui::command_line::CommandLine;

use super::engine::{
    self, find_closed_loop, Room, Storey, StyleLibrary,
    join::{self, JoinKind, JoinError},
    junction_solver::{self, WallJoinInput},
};
pub use super::engine::{Wall, WallJustification, WallLayer};
use super::engine::library::load_or_seed;
use super::engine::plan_view::{PhaseFilter, PlanPhase};
use super::engine::material::Material;
use super::engine::style::Style;
use super::engine::wall_style::{
    base_width_from_layers, effective_layers_for_wall_bb, migrate_gap_before_to_axis_offset, Layer,
    LayerFunction, LayerValue,
    ResolvedLayer, WallStyle,
};
use std::collections::HashMap;

/// APPID used for all AEC XDATA records (must stay stable for round-trip).
pub const AEC_APPID: &str = "OPENCAD_AEC";

/// In-memory storey store for the scaffold (persistence via document XDATA
/// is a follow-up; matches the former plugin's pragmatism).
static STOREYS: Mutex<Vec<Storey>> = Mutex::new(Vec::new());

/// User-visible notices queued when a stored [`join::JunctionOverride`] is
/// found to reference a layer/material that no longer exists and is
/// automatically cleaned up during regeneration (see
/// `validate_junction_override` / `remove_junction_override`). Regeneration
/// helpers (`regenerate_wall_representation_inner`, `join_junction_in_document`)
/// don't have direct access to a [`CommandLine`], so they queue the message
/// here; command entry points that do have one (e.g. `aec_walljoin_do`)
/// drain it via [`take_pending_override_warnings`] and surface it the same
/// way other non-fatal warnings are reported (`command_line.push_info`).
static PENDING_OVERRIDE_WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn queue_override_warning(msg: String) {
    if let Ok(mut q) = PENDING_OVERRIDE_WARNINGS.lock() {
        q.push(msg);
    }
}

/// Drain and return every queued override-invalidation notice since the last
/// call. Command entry points with a [`CommandLine`] should call this after
/// a regeneration/join and forward each message via `command_line.push_info`.
pub fn take_pending_override_warnings() -> Vec<String> {
    PENDING_OVERRIDE_WARNINGS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

fn layer_ref_matches(r: &join::LayerRef, set: &[join::LayerRef]) -> bool {
    set.iter().any(|l| {
        match (l.layer_id, r.layer_id) {
            (Some(a), Some(b)) => a == b,
            _ => l.material_id == r.material_id && l.role_tag == r.role_tag && l.index == r.index,
        }
    })
}

/// Merge a `ComponentRuleSet`'s slot-level `style_override` (checked in
/// `slots` priority order, first match per field wins) with its
/// `layer_style_override` entry matching `layer_ref` (fills any field the
/// slot override(s) left unset) — precedence tiers (a)/(b) of the Step 3
/// style resolution chain in `.junie/plans/aec-plan-view-display-variants.md`.
/// Returns an all-`None` [`engine::display_component::ComponentStyleOverride`]
/// when `rules` is `None` or nothing applies, so callers can always fall
/// through unconditionally to the `style_substitutions`/`hatch_override`/
/// material tiers below.
fn resolve_layer_style_override(
    rules: Option<&engine::display_component::ComponentRuleSet>,
    slots: &[engine::display_component::WallComponentSlot],
    layer_ref: &join::LayerRef,
) -> engine::display_component::ComponentStyleOverride {
    let mut out = engine::display_component::ComponentStyleOverride::default();
    let Some(rules) = rules else {
        return out;
    };
    // Detailed per-layer override first (more specific than a whole-slot
    // override), then each slot in priority order fills any remaining gaps.
    if let Some(lso) = rules.layer_style_override.iter().find(|lso| {
        match (lso.layer.layer_id, layer_ref.layer_id) {
            (Some(a), Some(b)) => a == b,
            _ => {
                lso.layer.material_id == layer_ref.material_id
                    && lso.layer.role_tag == layer_ref.role_tag
                    && lso.layer.index == layer_ref.index
            }
        }
    }) {
        out.line_type = lso.style.line_type.clone();
        out.line_color = lso.style.line_color;
        out.hatch_pattern = lso.style.hatch_pattern.clone();
        out.hatch_color = lso.style.hatch_color;
        out.hatch_scale = lso.style.hatch_scale;
        out.fill_color = lso.style.fill_color;
        out.hatch_angle = lso.style.hatch_angle;
        out.hatch_angle_relative = lso.style.hatch_angle_relative;
        out.cad_layer = lso.style.cad_layer.clone();
    }
    for slot in slots {
        if let Some(s) = rules.style_for(*slot) {
            if out.line_type.is_none() {
                out.line_type = s.line_type.clone();
            }
            if out.line_color.is_none() {
                out.line_color = s.line_color;
            }
            if out.hatch_pattern.is_none() {
                out.hatch_pattern = s.hatch_pattern.clone();
            }
            if out.hatch_color.is_none() {
                out.hatch_color = s.hatch_color;
            }
            if out.hatch_scale.is_none() {
                out.hatch_scale = s.hatch_scale;
            }
            if out.fill_color.is_none() {
                out.fill_color = s.fill_color;
            }
            if out.hatch_angle.is_none() {
                out.hatch_angle = s.hatch_angle;
            }
            if out.hatch_angle_relative.is_none() {
                out.hatch_angle_relative = s.hatch_angle_relative;
            }
            if out.cad_layer.is_none() {
                out.cad_layer = s.cad_layer.clone();
            }
        }
    }
    out
}

/// Result of resolving a [`PhaseFilter`] against a single wall's
/// [`PlanPhase`]: whether the wall should be shown at all under this
/// `DisplayConfig`, plus an optional extra style overlay (dashed lines for
/// `Demolition`, greyed-out for `Existing`, ...) to merge on top of the
/// wall's normally resolved style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhaseFilterResult {
    /// `false` means the wall must be hidden entirely for this config.
    pub visible: bool,
    /// Extra style overlay for `Demolition`/`Existing` walls; `None` for
    /// `New` walls or when the filter defines no overlay for this phase.
    pub extra_style: Option<engine::display_component::ComponentStyleOverride>,
}

/// Resolves a [`DisplayConfig::phase_filter`] against a wall's `phase`.
///
/// `filter == None` keeps every phase visible and applies built-in extras
/// (dashed demolition contour, grey existing contour). When a filter is
/// present, `phase` must be listed in `visible_phases`; `Demolition` /
/// `Existing` pick up `demolition_style`/`existing_style` or the same
/// built-in extras when those fields are unset.
fn default_phase_extra_style(
    phase: PlanPhase,
) -> Option<engine::display_component::ComponentStyleOverride> {
    match phase {
        PlanPhase::Demolition => Some(engine::display_component::ComponentStyleOverride {
            line_type: Some("DASHED".to_string()),
            ..Default::default()
        }),
        PlanPhase::Existing => Some(engine::display_component::ComponentStyleOverride {
            line_color: Some(acadrust::types::Color::Rgb {
                r: 136,
                g: 136,
                b: 136,
            }),
            ..Default::default()
        }),
        PlanPhase::New => None,
    }
}

pub fn apply_phase_filter(phase: PlanPhase, filter: Option<&PhaseFilter>) -> PhaseFilterResult {
    let Some(filter) = filter else {
        return PhaseFilterResult {
            visible: true,
            extra_style: default_phase_extra_style(phase),
        };
    };
    let visible = filter.visible_phases.contains(&phase);
    let extra_style = match phase {
        PlanPhase::Demolition => filter
            .demolition_style
            .clone()
            .or_else(|| default_phase_extra_style(phase)),
        PlanPhase::Existing => filter
            .existing_style
            .clone()
            .or_else(|| default_phase_extra_style(phase)),
        PlanPhase::New => None,
    };
    PhaseFilterResult {
        visible,
        extra_style,
    }
}

fn merge_phase_extra_into_rules(
    rules: &mut engine::display_component::ComponentRuleSet,
    extra: engine::display_component::ComponentStyleOverride,
) {
    let slot = engine::display_component::WallComponentSlot::Contour2D
        .key()
        .to_string();
    rules
        .style_override
        .entry(slot)
        .or_default()
        .overlay_from(&extra);
}

fn hide_all_display_slots(rules: &mut engine::display_component::ComponentRuleSet) {
    for slot in [
        engine::display_component::WallComponentSlot::Contour2D,
        engine::display_component::WallComponentSlot::Layers2D,
        engine::display_component::WallComponentSlot::ContourHatch2D,
        engine::display_component::WallComponentSlot::LayerHatch2D,
        engine::display_component::WallComponentSlot::Solid3D,
    ] {
        rules.visibility.insert(slot.key().to_string(), false);
    }
}

/// Build the [`join::LayerRef`] list for a wall's layer stack (given its
/// materials in layer order), keeping the `index` field aligned with each
/// layer's position — required so layers that reuse the same material (e.g.
/// two plaster layers) stay individually addressable by
/// [`join::LayerPairOverride`] instead of colliding.
fn layer_refs_from_materials<'a>(
    layers: impl IntoIterator<Item = (&'a str, Uuid)>,
) -> Vec<join::LayerRef> {
    layers
        .into_iter()
        .enumerate()
        .map(|(i, (m, id))| join::LayerRef {
            material_id: m.to_string(),
            role_tag: None,
            index: i,
            layer_id: Some(id),
        })
        .collect()
}

/// Validate a stored [`join::JunctionOverride`] against the current layer
/// sets of the wall(s) at the junction (`self_layers` and, when known,
/// `other_layers`): drop any [`join::LayerPairOverride`] whose `layer_a` or
/// `layer_b` (when `Some`) no longer matches an existing layer on either
/// side. Returns the cleaned override (`None` when nothing meaningful is
/// left — no `default_style` and no remaining valid `layer_pairs`) plus how
/// many pairs were dropped.
fn validate_junction_override(
    override_data: &join::JunctionOverride,
    self_layers: &[join::LayerRef],
    other_layers: &[join::LayerRef],
) -> (Option<join::JunctionOverride>, usize) {
    // `layer_a` identifies a layer on the wall that *owns* this override
    // (the resolver — see `resolve_layer_override_style` — only ever matches
    // it against that wall's own layer set); `layer_b`, when present, may
    // name a layer on either side of the junction.
    let matches_any = |r: &join::LayerRef| {
        layer_ref_matches(r, self_layers) || layer_ref_matches(r, other_layers)
    };
    let mut removed = 0usize;
    let kept_pairs: Vec<join::LayerPairOverride> = override_data
        .layer_pairs
        .iter()
        .filter(|p| {
            let ok = layer_ref_matches(&p.layer_a, self_layers)
                && match &p.layer_b {
                    Some(b) => matches_any(b),
                    None => true,
                };
            if !ok {
                removed += 1;
            }
            ok
        })
        .cloned()
        .collect();
    if removed == 0 {
        return (Some(override_data.clone()), 0);
    }
    if override_data.default_style.is_none() && kept_pairs.is_empty() {
        (None, removed)
    } else {
        (
            Some(join::JunctionOverride {
                default_style: override_data.default_style.clone(),
                layer_pairs: kept_pairs,
            }),
            removed,
        )
    }
}

/// Validate the [`join::JunctionOverride`] stored on `axis_handle`/`end_index`
/// against `self_layers`/`other_layers`, persisting the cleaned-up result (or
/// erasing the XDATA entirely) and queuing a user-visible notice when
/// anything was invalidated. Returns the override to actually use for this
/// regeneration (already cleaned).
fn validate_and_persist_junction_override(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: join::JunctionOverride,
    self_layers: &[join::LayerRef],
    other_layers: &[join::LayerRef],
) -> Option<join::JunctionOverride> {
    let (cleaned, removed) =
        validate_junction_override(&override_data, self_layers, other_layers);
    if removed == 0 {
        return cleaned;
    }
    match &cleaned {
        Some(ov) => {
            write_junction_override(scene, axis_handle, end_index, ov);
        }
        None => {
            remove_junction_override(scene, axis_handle, end_index);
        }
    }
    queue_override_warning(crate::tr!(
        "aec",
        "join-override-cleaned",
        removed = removed,
        axis = axis_handle.to_string()
    ));
    cleaned
}

/// Register `OPENCAD_AEC` in the APPID table if missing so XDATA survives
/// DWG/DXF round-trip.
fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// Attach (or replace) an `OPENCAD_AEC` XDATA record on `handle`.
///
/// When `record` starts with a string kind tag (e.g. `"WALL"`, `"OPENING"`),
/// only an existing AEC record with the same tag is replaced — other AEC
/// kinds on the same entity (notably `CHILD_HANDLES` / `JOINED_PEERS` owner
/// indexes) are preserved. Records without a leading string tag still replace
/// every AEC record (legacy behaviour).
fn write_aec_record(doc: &mut CadDocument, handle: Handle, record: ExtendedDataRecord) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(handle) else {
        return false;
    };
    let tag = match record.values.first() {
        Some(XDataValue::String(s)) => Some(s.as_str()),
        _ => None,
    };
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            match tag {
                Some(t) => {
                    !matches!(r.values.first(), Some(XDataValue::String(s)) if s == t)
                }
                None => false,
            }
        })
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    xd.add_record(record);
    if let Some(ah) = app_handle {
        let still_has_aec = xd.records().iter().any(|r| r.application_name == AEC_APPID);
        if !still_has_aec {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
    }
    true
}

/// Read the primary `OPENCAD_AEC` record on `entity` (WALL / OPENING / ROOM /
/// WALL_DERIVED / …), skipping pure index tags (`CHILD_HANDLES`,
/// `JOINED_PEERS`, `JOIN_OVERRIDE`) that may coexist on the same entity.
fn read_aec_record(entity: &EntityType) -> Option<&ExtendedDataRecord> {
    entity.common().extended_data.records().iter().find(|r| {
        if r.application_name != AEC_APPID {
            return false;
        }
        match r.values.first() {
            Some(XDataValue::String(s))
                if s == engine::owner_index::CHILD_HANDLES_TAG
                    || s == engine::owner_index::JOINED_PEERS_TAG
                    || s == JOIN_OVERRIDE_TAG =>
            {
                false
            }
            _ => true,
        }
    })
}

/// Display-child roles written on `WALL_REP` XDATA.
const WALL_REP_ROLE_CONTOUR: &str = "contour";
const WALL_REP_ROLE_HATCH: &str = "hatch";
const WALL_REP_ROLE_SOLID: &str = "solid";

/// Tag `handle` as a display child of the wall axis at `axis_handle`
/// (`WALL_REP` + owner handle + role). Legacy `WALL_DERIVED` is still
/// accepted by [`resolve_wall_package`].
fn write_wall_display_tag(scene: &mut Scene, handle: Handle, axis_handle: Handle, role: &str) {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("WALL_REP".to_string()));
    record.add_value(XDataValue::Handle(axis_handle));
    record.add_value(XDataValue::String(role.to_string()));
    write_aec_record(&mut scene.document, handle, record);
}

/// Legacy alias: untagged derived child (no role). Kept for tests that
/// synthesize orphan display entities.
fn write_wall_derived_tag(scene: &mut Scene, handle: Handle, axis_handle: Handle) {
    write_wall_display_tag(scene, handle, axis_handle, WALL_REP_ROLE_CONTOUR);
}

/// XDATA kind tag for a manual join-constraint override on a wall axis end
/// (a "junction"). A junction is identified by the wall axis's own handle
/// plus which end of its axis it sits at (`0` for the start vertex, `1` for
/// the last vertex) — mirroring the `end_a`/`end_b` vertex-index convention
/// already used by [`join::join_wall_axes`]. Multiple walls sharing a
/// junction point each store their own override on their own axis/end, since
/// XDATA lives on a single entity.
const JOIN_OVERRIDE_TAG: &str = "JOIN_OVERRIDE";

/// Write (or replace) a [`join::JunctionOverride`] as XDATA on `axis_handle`,
/// tied to `end_index` (`0` = axis start, `1` = axis end). The payload is
/// serialized as JSON, matching the pattern used for other AEC XDATA blobs.
/// Only the record for the same `end_index` is replaced — an override on the
/// other end of the same axis is left untouched.
pub fn write_junction_override(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: &join::JunctionOverride,
) -> bool {
    let Ok(json) = serde_json::to_string(override_data) else {
        return false;
    };
    ensure_app_id(&mut scene.document);
    let app_handle = scene
        .document
        .app_ids
        .get(AEC_APPID)
        .map(|a| a.handle.value());
    let Some(entity) = scene.document.get_entity_mut(axis_handle) else {
        return false;
    };
    let end_index = end_index as i32;
    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            if r.application_name != AEC_APPID {
                return true;
            }
            !matches!(
                (r.values.first(), r.values.get(1)),
                (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                    if s == JOIN_OVERRIDE_TAG && *e == end_index
            )
        })
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(JOIN_OVERRIDE_TAG.to_string()));
    record.add_value(XDataValue::Integer32(end_index));
    record.add_value(XDataValue::String(json));
    xd.add_record(record);
    if let Some(ah) = app_handle {
        let still_has_aec = xd.records().iter().any(|r| r.application_name == AEC_APPID);
        if !still_has_aec {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
    }
    true
}

/// Read the [`join::JunctionOverride`] stored on `axis_handle` for
/// `end_index` (`0` = axis start, `1` = axis end). Returns `None` when no
/// such XDATA exists — including on entities from drawings created before
/// this feature existed — and never panics on missing/malformed data.
pub fn read_junction_override(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Option<join::JunctionOverride> {
    let entity = scene.document.get_entity(axis_handle)?;
    let end_index = end_index as i32;
    entity
        .common()
        .extended_data
        .records()
        .iter()
        .find(|r| {
            r.application_name == AEC_APPID
                && matches!(
                    (r.values.first(), r.values.get(1)),
                    (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                        if s == JOIN_OVERRIDE_TAG && *e == end_index
                )
        })
        .and_then(|r| match r.values.get(2) {
            Some(XDataValue::String(json)) => serde_json::from_str(json).ok(),
            _ => None,
        })
}

/// Erase the [`join::JunctionOverride`] XDATA (if any) for `end_index` from
/// `axis_handle`. Used to fully clean up a degenerate override (no
/// `default_style` and no valid `layer_pairs` left) instead of persisting an
/// empty/meaningless record via [`write_junction_override`]. Returns `true`
/// when a record was actually removed.
pub fn remove_junction_override(scene: &mut Scene, axis_handle: Handle, end_index: usize) -> bool {
    let end_index = end_index as i32;
    let Some(entity) = scene.document.get_entity_mut(axis_handle) else {
        return false;
    };
    let xd = &mut entity.common_mut().extended_data;
    let before = xd.records().len();
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| {
            !(r.application_name == AEC_APPID
                && matches!(
                    (r.values.first(), r.values.get(1)),
                    (Some(XDataValue::String(s)), Some(XDataValue::Integer32(e)))
                        if s == JOIN_OVERRIDE_TAG && *e == end_index
                ))
        })
        .cloned()
        .collect();
    let changed = kept.len() != before;
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    changed
}

/// Overwrites only the `height` field of a wall's `WALL` XDATA record,
/// keeping `style_id`/`layers`/`storey_id`/`derived_handles`/`justification` intact.
/// Used by the Properties panel's editable "Height" row (single or
/// multi-selected walls).
pub fn write_wall_height(scene: &mut Scene, wall_handle: Handle, height: f64) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.height = height;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification, wall.phase, wall.hatch_override.as_ref()) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the layer-snapshot portion of a wall's `WALL` XDATA record,
/// keeping `style_id`/`height`/`storey_id`/`derived_handles`/`justification` intact.
pub fn write_wall_layers(scene: &mut Scene, wall_handle: Handle, layers: Vec<WallLayer>) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.layers = layers;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification, wall.phase, wall.hatch_override.as_ref()) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the `phase` field of a wall's `WALL` XDATA record,
/// keeping every other field intact. Used by the Properties panel's
/// editable "Phase" dropdown (single or multi-selected walls). Purely a
/// metadata edit — the wall's geometry is unaffected, so no regeneration
/// is triggered here.
pub fn write_wall_phase(scene: &mut Scene, wall_handle: Handle, phase: PlanPhase) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.phase = phase;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Overwrites only the `hatch_override` field of a wall's `WALL` XDATA
/// record, keeping every other field intact. Used by the Properties
/// panel's optional "Relativ zur Wand" checkbox + angle field (Step 6
/// hatch-angle chain: `Wall.hatch_override` > style-profile override >
/// `Material`). Passing `None` clears any existing per-wall override.
/// Purely a metadata edit — the wall's geometry is unaffected, so no
/// regeneration is triggered here.
pub fn write_wall_hatch_override(
    scene: &mut Scene,
    wall_handle: Handle,
    hatch_override: Option<engine::display_component::ComponentStyleOverride>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    wall.hatch_override = hatch_override;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification,
        wall.phase,
        wall.hatch_override.as_ref(),
    ) {
        record.add_value(v);
    }
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Every display child of `owner` (contour / hatch / solid): the persisted
/// `derived_handles` list plus a document scan for `WALL_REP` / `WALL_DERIVED`.
fn aec_value_as_handle(value: &XDataValue) -> Option<Handle> {
    match value {
        XDataValue::Handle(h) => Some(*h),
        XDataValue::Integer32(v) if *v >= 0 => Some(Handle::new(*v as u64)),
        XDataValue::Integer16(v) if *v >= 0 => Some(Handle::new(*v as u64)),
        XDataValue::String(s) => {
            let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            u64::from_str_radix(s, 16)
                .ok()
                .or_else(|| s.parse().ok())
                .map(Handle::new)
        }
        _ => None,
    }
}

fn wall_rep_owner_from_entity(entity: &EntityType) -> Option<Handle> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let kind = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if kind != "WALL_REP" && kind != "WALL_DERIVED" {
            continue;
        }
        return record.values.get(1).and_then(aec_value_as_handle);
    }
    None
}

fn collect_wall_display_children(scene: &Scene, owner: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(entity) = scene.document.get_entity(owner) {
        if let Some(wall) = wall_from_entity(entity) {
            out.extend(wall.derived_handles);
        }
    }
    for h in engine::owner_index::children_of(&scene.document, owner) {
        if !out.contains(&h) {
            out.push(h);
        }
    }
    for entity in scene.document.entities() {
        let handle = entity.common().handle;
        if handle == owner {
            continue;
        }
        if wall_rep_owner_from_entity(entity) == Some(owner) && !out.contains(&handle) {
            out.push(handle);
        }
    }
    out
}

/// Write `pl` into an existing contour handle when possible so the resident
/// wire cache retessellates the same outline instead of leaving a ghost.
fn reuse_or_add_wall_contour(
    scene: &mut Scene,
    reusable: &mut Vec<Handle>,
    pl: LwPolyline,
) -> Handle {
    while let Some(handle) = reusable.pop() {
        if scene.document.get_entity(handle).is_none() {
            continue;
        }
        if let Some(EntityType::LwPolyline(dst)) = scene.document.get_entity_mut(handle) {
            dst.vertices = pl.vertices;
            dst.is_closed = pl.is_closed;
            dst.elevation = pl.elevation;
        } else {
            scene.erase_entities(&[handle]);
            continue;
        }
        scene.bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
        return handle;
    }
    scene.add_entity(EntityType::LwPolyline(pl))
}

fn clicked_sample_xy(scene: &Scene, handle: Handle) -> Vec<(f64, f64)> {
    match scene.document.get_entity(handle) {
        Some(EntityType::LwPolyline(pl)) => {
            if pl.vertices.len() < 3 {
                return Vec::new();
            }
            pl.vertices
                .iter()
                .map(|v| (v.location.x, v.location.y))
                .collect()
        }
        Some(EntityType::Hatch(_)) => {
            let Some(model) = scene.hatches.get(&handle) else {
                return Vec::new();
            };
            let (ox, oy) = (model.world_origin[0], model.world_origin[1]);
            model
                .boundary
                .iter()
                .filter_map(|&[x, y]| {
                    if x.is_finite() && y.is_finite() {
                        Some((x as f64 + ox, y as f64 + oy))
                    } else {
                        None
                    }
                })
                .collect()
        }
        Some(EntityType::Solid3D(solid)) => {
            let mut samples = Vec::new();
            if let Some(mesh) = scene.meshes.get(&handle) {
                let [xmin, ymin, xmax, ymax] = mesh.world_aabb;
                if xmin.is_finite() && ymin.is_finite() && xmax.is_finite() && ymax.is_finite() {
                    samples.extend([
                        (xmin as f64, ymin as f64),
                        (xmax as f64, ymin as f64),
                        (xmax as f64, ymax as f64),
                        (xmin as f64, ymax as f64),
                        (((xmin + xmax) as f64) * 0.5, ((ymin + ymax) as f64) * 0.5),
                    ]);
                }
                if samples.len() < 3 {
                    if let Some(lod) = mesh.lods.first() {
                        samples.extend(lod.verts.iter().take(32).filter_map(|v| {
                            (v[0].is_finite() && v[1].is_finite()).then_some((v[0] as f64, v[1] as f64))
                        }));
                    }
                }
            }
            if samples.len() < 3 {
                for wire in &solid.wires {
                    for p in &wire.points {
                        if p.x.is_finite() && p.y.is_finite() {
                            samples.push((p.x, p.y));
                        }
                    }
                }
            }
            samples
        }
        _ => Vec::new(),
    }
}

fn resolve_wall_package_by_geometry(scene: &Scene, clicked: Handle) -> Option<Handle> {
    let samples = clicked_sample_xy(scene, clicked);
    if samples.len() < 3 {
        return None;
    }
    let mut best: Option<(Handle, f64)> = None;
    for entity in scene.document.entities() {
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let owner = entity.common().handle;
        if owner == clicked {
            continue;
        }
        let axis = get_wall_vertices(scene, owner);
        if axis.len() < 2 {
            continue;
        }
        let max_ok = wall.total_thickness().max(1e-6) * 1.25 + 1e-4;
        let mut worst = 0.0_f64;
        let mut all_near = true;
        for &(x, y) in &samples {
            let p = DVec3::new(x, y, 0.0);
            let mut dmin = f64::MAX;
            for w in axis.windows(2) {
                dmin = dmin.min(point_to_segment_dist_2d(p, w[0], w[1]));
            }
            if dmin > max_ok {
                all_near = false;
                break;
            }
            worst = worst.max(dmin);
        }
        if !all_near {
            continue;
        }
        match best {
            None => best = Some((owner, worst)),
            Some((_, d)) if worst < d => best = Some((owner, worst)),
            _ => {}
        }
    }
    best.map(|(h, _)| h)
}

pub fn resolve_wall_package(scene: &Scene, clicked: Handle) -> Handle {
    let Some(entity) = scene.document.get_entity(clicked) else {
        return clicked;
    };
    if wall_from_entity(entity).is_some() {
        return clicked;
    }
    if let Some(axis) = wall_rep_owner_from_entity(entity) {
        if scene.document.get_entity(axis).is_some() {
            return axis;
        }
    }
    // Fallback: the clicked entity is listed on a wall's derived_handles /
    // CHILD_HANDLES even if child XDATA was stripped (DWG round-trip / merge).
    for entity in scene.document.entities() {
        let owner = entity.common().handle;
        if let Some(wall) = wall_from_entity(entity) {
            if wall.derived_handles.iter().any(|h| *h == clicked) {
                return owner;
            }
        }
        if engine::owner_index::children_of(&scene.document, owner)
            .iter()
            .any(|h| *h == clicked)
            && wall_from_entity(entity).is_some()
        {
            return owner;
        }
    }
    if let Some(owner) = resolve_wall_package_by_geometry(scene, clicked) {
        return owner;
    }
    clicked
}

/// Expand `handles` so each wall owner is accompanied by its display children.
/// Derived picks resolve to the owner first; non-wall handles pass through.
/// Used by hover, selection highlight, and MOVE/transform so the package
/// always travels together.
pub fn expand_handles_for_wall_packages(scene: &Scene, handles: &[Handle]) -> Vec<Handle> {
    let mut out = Vec::with_capacity(handles.len());
    let mut seen = rustc_hash::FxHashSet::default();
    for &handle in handles {
        let owner = resolve_wall_package(scene, handle);
        for package in wall_package_handles(scene, owner) {
            if seen.insert(package) {
                out.push(package);
            }
        }
    }
    out
}

/// True when `handle` is a wall-derived contour/hatch/solid entity — i.e. a
/// visible-layer entity generated by [`regenerate_wall_representation`] that
/// carries a `WALL_DERIVED` XDATA record pointing back at its axis. Used by
/// [`wall_axis_snap_wires`] to keep such entities out of the generic snap
/// candidate set (Bug 2): walls must always connect at their axes, never at
/// a shell/hatch/solid outline.
pub fn is_wall_derived_non_axis(scene: &Scene, handle: Handle) -> bool {
    resolve_wall_package(scene, handle) != handle
}

/// True when `handle` is the wall axis itself — the (normally invisible,
/// `AEC_WALL_AXIS_LAYER`) `LwPolyline` carrying the `WALL` XDATA
/// record. Axis entities must remain snap candidates even though their
/// layer is turned off (Bug 2).
fn is_wall_axis_entity(entity: &EntityType) -> bool {
    entity.common().layer == AEC_WALL_AXIS_LAYER && wall_from_entity(entity).is_some()
}

/// Snap-candidate carve-out for AEC walls (Bug 2).
///
/// The generic wire set used for OSNAP (`Scene::hit_test_wires` /
/// `Scene::entity_wires_arc`) is the same set the renderer draws from, so it
/// naturally excludes entities on the off/invisible `AEC_WALL_AXIS_LAYER`
/// (the wall axis) while including every visible wall-derived contour/
/// hatch/solid. Left alone, that means:
/// - the wall axis can never be snapped to at all, and
/// - a new/edited wall would snap onto the neighbour's shell/hatch outline
///   instead of its axis, producing walls that don't actually connect at a
///   shared axis point.
///
/// This filters `wires` down to only the axis for any wall-derived entity
/// (dropping its shell/hatch/solid siblings) and re-tessellates the axis
/// entities that the resident wire set skips outright, so they participate
/// in snapping despite their layer being off. Non-wall geometry passes
/// through completely unchanged. Cheap no-op when the document has no
/// `AEC_WALL_AXIS_LAYER` (i.e. no AEC walls have ever been drawn).
pub fn wall_axis_snap_wires(
    scene: &Scene,
    wires: std::sync::Arc<Vec<WireModel>>,
) -> std::sync::Arc<Vec<WireModel>> {
    if scene.document.layers.get(AEC_WALL_AXIS_LAYER).is_none() {
        return wires;
    }
    let mut filtered: Vec<WireModel> = wires
        .iter()
        .filter(|w| match w.name.parse::<u64>() {
            Ok(v) => !is_wall_derived_non_axis(scene, Handle::new(v)),
            Err(_) => true,
        })
        .cloned()
        .collect();

    let axis_entities: Vec<EntityType> = scene
        .document
        .entities()
        .filter(|e| is_wall_axis_entity(e))
        .cloned()
        .collect();
    if !axis_entities.is_empty() {
        filtered.extend(scene.wires_for_entities(&axis_entities));
    }
    std::sync::Arc::new(filtered)
}

/// Extracts vertices from a wall's axis polyline.
pub(crate) fn get_wall_vertices(scene: &Scene, handle: Handle) -> Vec<DVec3> {
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(handle) {
        pl.vertices.iter().map(|v| DVec3::new(v.location.x, v.location.y, 0.0)).collect()
    } else {
        Vec::new()
    }
}

pub(crate) fn get_wall_bulges(scene: &Scene, handle: Handle) -> Vec<f64> {
    if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(handle) {
        pl.vertices.iter().map(|v| v.bulge).collect()
    } else {
        Vec::new()
    }
}

/// Updates a wall's axis polyline vertices.
pub(crate) fn update_wall_vertices(scene: &mut Scene, handle: Handle, vertices: &[DVec3]) {
    if let Some(entity) = scene.document.get_entity_mut(handle) {
        if let EntityType::LwPolyline(pl) = entity {
            // Keep bulge / start-width when only endpoint positions change so
            // a join/extend cannot leave a 2D contour with stale arc data.
            if pl.vertices.len() == vertices.len() {
                let old: Vec<(f64, f64, f64)> = pl
                    .vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y, v.bulge))
                    .collect();
                for (dst, src) in pl.vertices.iter_mut().zip(vertices.iter()) {
                    dst.location = acadrust::types::Vector2::new(src.x, src.y);
                }
                let n = pl.vertices.len();
                for i in 0..n.saturating_sub(1) {
                    let bulge = old[i].2;
                    if bulge.abs() <= 1e-12 {
                        continue;
                    }
                    let new_b = engine::arc::retarget_bulge(
                        (old[i].0, old[i].1),
                        (old[i + 1].0, old[i + 1].1),
                        bulge,
                        (vertices[i].x, vertices[i].y),
                        (vertices[i + 1].x, vertices[i + 1].y),
                    );
                    pl.vertices[i].bulge = new_b;
                }
            } else {
                pl.vertices = vertices
                    .iter()
                    .map(|v| {
                        acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(v.x, v.y))
                    })
                    .collect();
            }
        }
    }
}

/// Collect baseline segments of every `WALL`-tagged `LwPolyline`.
fn collect_wall_segments(doc: &CadDocument) -> Vec<((f64, f64), (f64, f64))> {
    let mut segments = Vec::new();
    for entity in doc.entities() {
        let EntityType::LwPolyline(pl) = entity else {
            continue;
        };
        let is_wall = matches!(
            read_aec_record(entity).and_then(|r| r.values.first()),
            Some(XDataValue::String(kind)) if kind == "WALL"
        );
        if !is_wall {
            continue;
        }
        for pair in pl.vertices.windows(2) {
            let a = (pair[0].location.x, pair[0].location.y);
            let b = (pair[1].location.x, pair[1].location.y);
            segments.push((a, b));
        }
        if pl.is_closed {
            if let (Some(first), Some(last)) = (pl.vertices.first(), pl.vertices.last()) {
                segments.push((
                    (last.location.x, last.location.y),
                    (first.location.x, first.location.y),
                ));
            }
        }
    }
    segments
}
/// Layer name used for the (invisible) wall axis / centerline reference
/// geometry. `AEC_ROOM` / loop-detection and the `WALL` XDATA carrier keep
/// living on this layer once the visible contour/hatch/solid representation
/// is regenerated.
pub const AEC_WALL_AXIS_LAYER: &str = "AEC_WALL_AXIS";
/// Build a `WALL` XDATA record's values.
///
/// `derived_handles` is appended as a trailing `count` + `Handle` block so
/// records written before this field existed (no trailing block) still
/// parse back with an empty list.
pub fn wall_record(
    style_id: &str,
    height: f64,
    storey_id: u32,
    layers: &[WallLayer],
    derived_handles: &[Handle],
    justification: WallJustification,
    phase: PlanPhase,
    hatch_override: Option<&engine::display_component::ComponentStyleOverride>,
) -> Vec<XDataValue> {
    let mut values = Vec::new();
    values.push(XDataValue::String("WALL".to_string()));
    values.push(XDataValue::String(style_id.to_string()));
    values.push(XDataValue::Distance(height));
    values.push(XDataValue::Integer32(storey_id as i32));
    values.push(XDataValue::Integer32(layers.len() as i32));
    for layer in layers {
        values.push(XDataValue::String(layer.material.clone()));
        values.push(XDataValue::Distance(layer.thickness));
        values.push(XDataValue::String(layer.function.clone()));
    }
    values.push(XDataValue::Integer32(derived_handles.len() as i32));
    for h in derived_handles {
        values.push(XDataValue::Handle(*h));
    }
    values.push(XDataValue::String(justification.as_str().to_string()));

    // Schema marker distinguishing absolute axis_offset extras from legacy
    // gap_before stacking values. Absent on older records.
    values.push(XDataValue::String("axis_offset".to_string()));

    // Trailing layer extras: axis_offset, bottom_offset, top_offset for each layer
    for layer in layers {
        values.push(XDataValue::Distance(layer.axis_offset));
        values.push(XDataValue::Distance(layer.bottom_offset));
        values.push(XDataValue::Distance(layer.top_offset));
    }
    // Trailing layer_override (drawing-layer override) for each layer, as an
    // empty string standing for `None`; appended last so records written
    // before this field existed (no trailing block) still parse back with
    // every layer defaulting to `None`.
    for layer in layers {
        values.push(XDataValue::String(layer.layer_override.clone().unwrap_or_default()));
    }
    // Trailing hatch_override (per-layer hatch pattern override) for each
    // layer, same empty-string-for-`None` convention, appended after
    // `layer_override` so records written before this field existed still
    // parse back with every layer defaulting to `None`.
    for layer in layers {
        values.push(XDataValue::String(layer.hatch_override.clone().unwrap_or_default()));
    }
    // Trailing layer_id (stable identity) for each layer, preceded by a
    // tag so it can be reliably distinguished from other trailing extras.
    values.push(XDataValue::String("layer_id".to_string()));
    for layer in layers {
        values.push(XDataValue::String(layer.layer_id.to_string()));
    }
    // Trailing `phase` tag, appended last so records written before this
    // field existed still parse back defaulting to `PlanPhase::New`.
    values.push(XDataValue::String(phase.as_str().to_string()));
    // Trailing per-wall-instance `hatch_override` (Step 3 hatch-angle chain:
    // `Wall.hatch_override` > style-profile override > `Material`), appended
    // last so records written before this field existed still parse back
    // with `None`. Encoded as a presence flag followed by the two hatch
    // fields (angle in degrees as a `Distance`, relative-flag as a string),
    // each using a sentinel when unset (`f64::NAN` / empty string).
    match hatch_override {
        Some(ov) => {
            values.push(XDataValue::String("1".to_string()));
            values.push(XDataValue::Distance(ov.hatch_angle.unwrap_or(f64::NAN)));
            values.push(XDataValue::String(match ov.hatch_angle_relative {
                Some(true) => "true".to_string(),
                Some(false) => "false".to_string(),
                None => String::new(),
            }));
        }
        None => {
            values.push(XDataValue::String("0".to_string()));
            values.push(XDataValue::Distance(f64::NAN));
            values.push(XDataValue::String(String::new()));
        }
    }
    values
}

/// Parse a `WALL` XDATA record back into a [`Wall`].
pub fn wall_from_entity(entity: &EntityType) -> Option<Wall> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "WALL" {
        return None;
    }

    let style_id = if let XDataValue::String(s) = &v[1] {
        s.clone()
    } else {
        return None;
    };
    let height = if let XDataValue::Distance(d) = v[2] {
        d
    } else {
        return None;
    };
    let storey_id = if let XDataValue::Integer32(i) = v[3] {
        i as u32
    } else {
        return None;
    };
    let layer_count = if let XDataValue::Integer32(i) = v[4] {
        i as usize
    } else {
        return None;
    };

    if v.len() < 5 + layer_count * 3 {
        return None;
    }

    let mut layers = Vec::with_capacity(layer_count);
    for i in 0..layer_count {
        let base = 5 + i * 3;
        let mat = if let XDataValue::String(s) = &v[base] {
            s.clone()
        } else {
            return None;
        };
        let thick = if let XDataValue::Distance(d) = v[base + 1] {
            d
        } else {
            return None;
        };
        let func = if let XDataValue::String(s) = &v[base + 2] {
            s.clone()
        } else {
            return None;
        };
        layers.push(WallLayer {
            material: mat,
            thickness: thick,
            function: func,
            axis_offset: 0.0,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
            // Placeholder; overwritten below from the "layer_id" tag when
            // present, or assigned a fresh stable ID otherwise (legacy wall).
            layer_id: Uuid::nil(),
        });
    }

    // Trailing `derived_handles` block: absent on records written before
    // this field existed, so a missing/short tail just means "none".
    let tail = 5 + layer_count * 3;
    let mut derived_handles = Vec::new();
    let mut justification = WallJustification::Center;
    let mut phase = PlanPhase::default();
    let mut wall_hatch_override: Option<engine::display_component::ComponentStyleOverride> = None;
    let mut extras_applied = false;

    if v.len() > tail {
        if let XDataValue::Integer32(count) = v[tail] {
            let count = count.max(0) as usize;
            if v.len() >= tail + 1 + count {
                for i in 0..count {
                    if let XDataValue::Handle(h) = v[tail + 1 + i] {
                        derived_handles.push(h);
                    }
                }
                // Justification might follow derived handles
                if v.len() > tail + 1 + count {
                    if let XDataValue::String(s) = &v[tail + 1 + count] {
                        justification = WallJustification::from_str(s);
                    }

                    // Optional layer extras (axis_offset/gap + vertical offsets) might follow justification.
                    // New records insert a "axis_offset" schema marker before the triples;
                    // legacy records store gap_before and must be migrated to absolute offsets.
                    let mut extras_tail = tail + 1 + count + 1;
                    let mut legacy_gap_before = true;
                    if v.len() > extras_tail {
                        if let XDataValue::String(marker) = &v[extras_tail] {
                            if marker == "axis_offset" {
                                legacy_gap_before = false;
                                extras_tail += 1;
                            }
                        }
                    }
                    if v.len() >= extras_tail + layer_count * 3 {
                        let mut legacy_gaps = vec![0.0; layer_count];
                        for i in 0..layer_count {
                            let base = extras_tail + i * 3;
                            if let XDataValue::Distance(g) = v[base] {
                                if legacy_gap_before {
                                    legacy_gaps[i] = g;
                                } else {
                                    layers[i].axis_offset = g;
                                }
                            }
                            if let XDataValue::Distance(b) = v[base + 1] {
                                layers[i].bottom_offset = b;
                            }
                            if let XDataValue::Distance(tv) = v[base + 2] {
                                layers[i].top_offset = tv;
                            }
                        }
                        if legacy_gap_before {
                            let pairs: Vec<(f64, f64)> = layers
                                .iter()
                                .zip(legacy_gaps.iter())
                                .map(|(l, g)| (l.thickness, *g))
                                .collect();
                            let offsets = migrate_gap_before_to_axis_offset(&pairs);
                            for (layer, offset) in layers.iter_mut().zip(offsets) {
                                layer.axis_offset = offset;
                            }
                        }
                        extras_applied = true;

                        // Optional layer_override block might follow the axis_offset/offset extras.
                        let override_tail = extras_tail + layer_count * 3;
                        if v.len() >= override_tail + layer_count {
                            for i in 0..layer_count {
                                if let XDataValue::String(s) = &v[override_tail + i] {
                                    layers[i].layer_override =
                                        if s.is_empty() { None } else { Some(s.clone()) };
                                }
                            }

                            // Optional hatch_override block might follow layer_override.
                            let hatch_tail = override_tail + layer_count;
                            if v.len() >= hatch_tail + layer_count {
                                for i in 0..layer_count {
                                    if let XDataValue::String(s) = &v[hatch_tail + i] {
                                        layers[i].hatch_override =
                                            if s.is_empty() { None } else { Some(s.clone()) };
                                    }
                                }

                                // Search for optional "layer_id" tag in trailing values,
                                // and note how many slots it occupies (tag + one UUID
                                // string per layer) so the subsequent `phase` tag can
                                // be located at the correct offset. Absent on records
                                // written before this field existed.
                                let mut layer_id_block_len = 0usize;
                                if hatch_tail + layer_count < v.len() {
                                    if let XDataValue::String(s) = &v[hatch_tail + layer_count] {
                                        if s == "layer_id" && hatch_tail + layer_count + 1 + layer_count <= v.len() {
                                            for i in 0..layer_count {
                                                if let XDataValue::String(id_str) =
                                                    &v[hatch_tail + layer_count + 1 + i]
                                                {
                                                    if let Ok(id) = Uuid::parse_str(id_str) {
                                                        layers[i].layer_id = id;
                                                    }
                                                }
                                            }
                                            layer_id_block_len = 1 + layer_count;
                                        }
                                    }
                                }

                                // Optional trailing `phase` tag might follow the
                                // hatch_override block (and the layer_id block, if
                                // present). Absent on older records, which default
                                // to `PlanPhase::New`.
                                let phase_tail = hatch_tail + layer_count + layer_id_block_len;
                                if v.len() > phase_tail {
                                    if let XDataValue::String(s) = &v[phase_tail] {
                                        phase = PlanPhase::from_str(s);
                                    }

                                    // Optional trailing per-wall-instance
                                    // `hatch_override` block might follow `phase`
                                    // (presence flag + angle + relative-flag).
                                    // Absent on older records, which default to
                                    // `None` (no per-wall override).
                                    let wall_hatch_tail = phase_tail + 1;
                                    if v.len() >= wall_hatch_tail + 3 {
                                        if let XDataValue::String(flag) = &v[wall_hatch_tail] {
                                            if flag == "1" {
                                                let angle = if let XDataValue::Distance(a) = v[wall_hatch_tail + 1] {
                                                    if a.is_nan() { None } else { Some(a) }
                                                } else {
                                                    None
                                                };
                                                let relative = if let XDataValue::String(r) = &v[wall_hatch_tail + 2] {
                                                    match r.as_str() {
                                                        "true" => Some(true),
                                                        "false" => Some(false),
                                                        _ => None,
                                                    }
                                                } else {
                                                    None
                                                };
                                                wall_hatch_override = Some(engine::display_component::ComponentStyleOverride {
                                                    hatch_angle: angle,
                                                    hatch_angle_relative: relative,
                                                    ..Default::default()
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Records without layer extras predate both gap_before and axis_offset
    // fields; treat them as gap_before = 0 and migrate to a centered stack so
    // geometry stays identical to the historical default.
    if !extras_applied && !layers.is_empty() {
        let pairs: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, 0.0)).collect();
        let offsets = migrate_gap_before_to_axis_offset(&pairs);
        for (layer, offset) in layers.iter_mut().zip(offsets) {
            layer.axis_offset = offset;
        }
    }

    // Assign new IDs to legacy layers that lack a stable identity from XDATA.
    for l in &mut layers {
        if l.layer_id.is_nil() {
            l.layer_id = Uuid::new_v4();
        }
    }

    Some(Wall {
        style_id,
        height,
        storey_id,
        layers,
        derived_handles,
        justification,
        phase,
        hatch_override: wall_hatch_override,
    })
}

/// Reconstruct a [`StyleLibrary`] from every wall in `scene`.
pub fn extract_style_library_from_scene(scene: &Scene) -> StyleLibrary {
    let walls: Vec<Wall> = scene
        .document
        .entities()
        .filter_map(wall_from_entity)
        .collect();
    engine::library::extract_style_library_from_walls(&walls)
}

/// Helper to get total thickness, height, and storey_id for a wall entity.
pub fn wall_thickness_and_height(entity: &EntityType) -> Option<(f64, f64, u32)> {
    let wall = wall_from_entity(entity)?;
    Some((wall.total_thickness(), wall.height, wall.storey_id))
}

/// Parameters for a 3D extrusion of a wall layer.
///
/// Contains the 2D footprint (a closed polygon loop) and the height
/// to extrude it by.
#[derive(Debug, Clone, PartialEq)]
pub struct WallLayerExtrusion {
    pub footprint: Vec<(f64, f64)>,
    pub height: f64,
    pub base_offset: f64,
}

/// Extract open-axis centerline points and per-vertex LWPOLYLINE bulges from a
/// wall axis entity. Returns empty vectors when `wall_entity` is not an
/// `LwPolyline`.
fn wall_axis_points_and_bulges(wall_entity: &EntityType) -> (Vec<(f64, f64)>, Vec<f64>) {
    let EntityType::LwPolyline(pl) = wall_entity else {
        return (Vec::new(), Vec::new());
    };
    let centerline: Vec<(f64, f64)> = pl
        .vertices
        .iter()
        .map(|v| (v.location.x, v.location.y))
        .collect();
    let bulges: Vec<f64> = pl.vertices.iter().map(|v| v.bulge).collect();
    (centerline, bulges)
}

/// Sample a bulge segment (straight or arc) into `segments + 1` points for
/// wireframe preview rendering. Falls back to the two endpoints for a
/// straight (`bulge ≈ 0`) or degenerate segment.
fn tessellate_bulge_segment(
    start: (f64, f64),
    end: (f64, f64),
    bulge: f64,
    segments: usize,
) -> Vec<(f64, f64)> {
    let Some(arc) = engine::arc::bulge_to_arc(start, end, bulge) else {
        return vec![start, end];
    };
    let included = arc.included_angle();
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let angle = if arc.ccw {
                arc.start_angle + included * t
            } else {
                arc.start_angle - included * t
            };
            (
                arc.center.0 + arc.radius * angle.cos(),
                arc.center.1 + arc.radius * angle.sin(),
            )
        })
        .collect()
}

/// Extracts a wall's centerline points from its [`LwPolyline`] geometry and
/// computes parallel boundary lines for each layer.
///
/// Returns one `(inner, outer)` boundary pair per layer. Prefer
/// [`wall_layer_footprints`] / [`engine::representation::build_wall_representation`]
/// when closed per-layer polygons are enough — this pair form is kept for
/// callers that still need the raw offset polylines (tests, miter diagnostics).
///
/// Axis bulges (arc segments) are honoured via
/// [`engine::contour::layer_contours_with_bulges`].
pub fn wall_layer_contour_polylines(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<(Vec<(f64, f64)>, Vec<(f64, f64)>)> {
    let (centerline, bulges) = wall_axis_points_and_bulges(wall_entity);
    if centerline.len() < 2 {
        return Vec::new();
    }
    let layer_data: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    engine::contour::layer_contours_with_bulges(&centerline, &bulges, &layer_data)
        .into_iter()
        .map(|(a, b)| (a.points, b.points))
        .collect()
}

/// Closed per-layer footprints for a wall axis entity, derived via the shared
/// [`engine::representation::WallRepresentation`] builder.
///
/// Axis is assumed already justification-shifted (centerline_offset = 0),
/// matching how walls are persisted after draw. Axis bulges are forwarded so
/// curved walls produce arc-aware offset footprints.
pub fn wall_layer_footprints(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<Vec<(f64, f64)>> {
    wall_layer_footprints_with_bulges(wall_entity, layers)
        .into_iter()
        .map(|(pts, _)| pts)
        .collect()
}

/// Like [`wall_layer_footprints`], but also returns per-vertex bulges for each
/// closed footprint (LWPOLYLINE convention) so derived contour entities can
/// keep arc segments exact.
pub fn wall_layer_footprints_with_bulges(
    wall_entity: &EntityType,
    layers: &[WallLayer],
) -> Vec<(Vec<(f64, f64)>, Vec<f64>)> {
    let (centerline, bulges) = wall_axis_points_and_bulges(wall_entity);
    if centerline.len() < 2 {
        return Vec::new();
    }
    let layer_data: Vec<(f64, f64)> = layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    let repr = engine::representation::build_wall_representation_with_bulges(
        &centerline,
        &bulges,
        &layer_data,
        0.0,
    );
    repr.layer_contours_2d
        .into_iter()
        .zip(repr.layer_contour_bulges.into_iter())
        .collect()
}

/// Produces the parameters needed to create an extruded solid for each wall layer.
///
/// This implementation uses the "layer footprint" approach: it builds a closed
/// 2D polygon per layer via [`engine::representation::build_wall_representation`]
/// and returns it along with the wall height.
///
/// Scoping Decision: This function returns plain data ([`WallLayerExtrusion`]).
/// A future step can wire this to the host's `Solid3D` entity creation calls
/// (e.g., using `sweep_model::extruded`).
pub fn wall_layer_extrusions(
    wall_entity: &EntityType,
    layers: &[WallLayer],
    height: f64,
) -> Vec<WallLayerExtrusion> {
    let footprints = wall_layer_footprints(wall_entity, layers);
    if footprints.is_empty() {
        return Vec::new();
    }

    let mut extrusions = Vec::with_capacity(layers.len());
    for (i, footprint) in footprints.into_iter().enumerate() {
        let layer = &layers[i];
        let effective_height = (height - layer.bottom_offset - layer.top_offset).max(0.0);
        let base_offset = layer.bottom_offset;

        extrusions.push(WallLayerExtrusion {
            footprint,
            height: effective_height,
            base_offset,
        });
    }
    extrusions
}

/// Register the `AEC_WALL_AXIS` layer (invisible / non-printable) if it
/// isn't already in the document's layer table.
pub fn ensure_wall_axis_layer(scene: &mut Scene) {
    set_wall_axis_layer_visible(scene, false);
}

/// Session override: show or hide every wall axis via `AEC_WALL_AXIS`.
/// The layer stays non-plottable. OSNAP still injects axis wires when off.
pub fn set_wall_axis_layer_visible(scene: &mut Scene, visible: bool) {
    scene.ensure_layer(AEC_WALL_AXIS_LAYER);
    let changed = if let Some(layer) = scene.document.layers.get_mut(AEC_WALL_AXIS_LAYER) {
        layer.is_plottable = false;
        let was_off = layer.flags.off;
        layer.flags.off = !visible;
        was_off != !visible
    } else {
        false
    };
    if changed {
        scene.invalidate_layer_dependencies(&[AEC_WALL_AXIS_LAYER.to_string()]);
    }
}

/// Idle visibility of the axis layer from a resolved rule set. Absent rules
/// keep the historical default (layer off).
pub fn axis_visible_from_rules(
    rules: Option<&engine::display_component::ComponentRuleSet>,
) -> bool {
    use engine::display_component::WallComponentSlot;
    rules.is_some_and(|r| r.is_visible(WallComponentSlot::AxisLine))
}

/// Erase draw-time companion entities that are superseded once
/// [`regenerate_wall_representation`] builds proper `WALL_REP` children.
///
/// `WallCommand` commits an untagged outer-contour `LwPolyline` alongside the
/// axis for live preview. That polyline is **not** a `WALL_REP` child and is
/// never refreshed on axis edits — leaving it in the document produces the
/// "orphan 2D polyline that doesn't follow wall changes" symptom. Call this
/// with every non-axis live handle when the wall drawing command finishes,
/// immediately before regeneration.
pub fn erase_wall_live_preview_companions(
    scene: &mut Scene,
    axis_handle: Handle,
    companions: &[Handle],
) {
    let mut to_erase = Vec::new();
    for &h in companions {
        if h == axis_handle {
            continue;
        }
        let Some(entity) = scene.document.get_entity(h) else {
            continue;
        };
        // Drop the rubber-band contour *and* a superseded live axis: each
        // segment currently `CommitEntity`s a *new* WALL polyline while the
        // draw-time live axis stays in the document as a visible 2D line.
        if !matches!(entity, EntityType::LwPolyline(_)) {
            continue;
        }
        if is_wall_display_child_entity(entity) {
            continue;
        }
        to_erase.push(h);
    }
    if !to_erase.is_empty() {
        scene.erase_entities(&to_erase);
    }
}

/// True when `entity` carries a `WALL_REP` / `WALL_DERIVED` display tag.
fn is_wall_display_child_entity(entity: &EntityType) -> bool {
    let Some(record) = read_aec_record(entity) else {
        return false;
    };
    matches!(
        record.values.first(),
        Some(XDataValue::String(kind)) if kind == "WALL_REP" || kind == "WALL_DERIVED"
    )
}

/// Number of straight segments used to approximate one arc edge when
/// tessellating a closed footprint ring for the hatch fill boundary (see
/// [`tessellate_ring_with_bulges`]). [`HatchModel::boundary`] is a plain
/// point list with no bulge support, unlike the `LwPolyline` contour entity,
/// so a curved wall layer's hatch would otherwise fill only the straight
/// chord between an arc segment's endpoints instead of following the curve.
const WALL_HATCH_ARC_SEGMENTS: usize = 24;

/// Expand a closed footprint ring (LWPOLYLINE-style vertex + per-edge bulge)
/// into a dense polyline so arc segments are approximated by short straight
/// chords instead of a single long one.
///
/// Returns `ring` unchanged (cloned) when every bulge is zero (or absent),
/// so straight-only walls keep the exact same hatch boundary as before this
/// tessellation was added — no behavior change, no performance cost.
fn retarget_closed_footprint_bulges(
    original: &[(f64, f64)],
    original_bulges: &[f64],
    edited: &[(f64, f64)],
) -> Vec<f64> {
    let n = edited.len();
    if n < 2 {
        return vec![0.0; n];
    }
    if original.len() != n {
        return vec![0.0; n];
    }
    let mut out = vec![0.0; n];
    for i in 0..n {
        let bulge = original_bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() <= 1e-12 {
            continue;
        }
        let j = (i + 1) % n;
        out[i] = engine::arc::retarget_bulge(
            original[i],
            original[j],
            bulge,
            edited[i],
            edited[j],
        );
    }
    out
}

fn tessellate_ring_with_bulges(ring: &[(f64, f64)], bulges: &[f64]) -> Vec<(f64, f64)> {
    if ring.len() < 3 || !bulges.iter().any(|b| b.abs() > 1e-12) {
        return ring.to_vec();
    }
    let n = ring.len();
    let mut out = Vec::with_capacity(n * 2);
    for i in 0..n {
        let start = ring[i];
        let end = ring[(i + 1) % n];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        if bulge.abs() <= 1e-12 {
            out.push(start);
            continue;
        }
        // `tessellate_bulge_segment` includes both endpoints; drop the last
        // sample (shared with the next edge's start) to avoid duplicates.
        let mut samples = tessellate_bulge_segment(start, end, bulge, WALL_HATCH_ARC_SEGMENTS);
        samples.pop();
        out.extend(samples);
    }
    out
}

/// Pack a single closed 2D ring into a [`HatchModel`]'s relative-boundary
/// representation (anchor at the ring's first vertex, f32 offsets from it),
/// mirroring the shape `pack_rings` builds in `draw/hatch.rs` for a single
/// loop with no holes.
fn pack_wall_ring(ring: &[(f64, f64)]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let origin = ring.first().copied().unwrap_or((0.0, 0.0));
    let origin = [origin.0, origin.1];
    let mut rel: Vec<[f32; 2]> = ring
        .iter()
        .map(|&(x, y)| [(x - origin[0]) as f32, (y - origin[1]) as f32])
        .collect();
    let mut wcs: Vec<[f64; 2]> = ring.iter().map(|&(x, y)| [x, y]).collect();
    // Close the loop so hatch tessellation matches the closed 2D contour.
    if rel.len() >= 3 {
        let first = rel[0];
        let last = *rel.last().unwrap();
        if (first[0] - last[0]).abs() > 1e-6 || (first[1] - last[1]).abs() > 1e-6 {
            rel.push(first);
            wcs.push(wcs[0]);
        }
    }
    (rel, origin, wcs)
}

/// Convert an `AcadColor` into a normalized RGBA color with the alpha the
/// AEC hatch representation uses. Logical colours without a resolvable RGB
/// fall back to light grey.
fn wall_hatch_color(color: acadrust::types::Color) -> [f32; 4] {
    let (r, g, b) = color.rgb().unwrap_or((153, 153, 153));
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.85]
}

/// Convert a material `line_color` 0xRRGGBB into an `AcadColor::Rgb`.
fn material_line_color_as_acad(rgb: u32) -> acadrust::types::Color {
    acadrust::types::Color::Rgb {
        r: ((rgb >> 16) & 0xFF) as u8,
        g: ((rgb >> 8) & 0xFF) as u8,
        b: (rgb & 0xFF) as u8,
    }
}

/// Error returned by [`regenerate_wall_representation`]; never a panic —
/// malformed axis/XDATA just leaves the wall without a rebuilt
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallRegenError {
    /// `wall_handle` doesn't resolve to an entity carrying `WALL`
    /// XDATA.
    NotAWall,
    /// The wall has no material layers to build a representation from.
    NoLayers,
    /// The axis geometry didn't yield usable layer contours (e.g. fewer
    /// than two vertices).
    NoContours,
}

/// (Re)build the visible 2D contour + hatch and 3D solid representation for
/// the wall at `wall_handle`, from its axis polyline + `WALL`
/// XDATA.
///
/// The axis polyline is moved onto the invisible `AEC_WALL_AXIS` layer (kept
/// as reference geometry for `AEC_ROOM` / loop detection and as the XDATA
/// carrier). Every entity handle previously recorded in `derived_handles` is
/// erased first, so calling this repeatedly on the same wall never
/// accumulates duplicates. Derived handles are always persisted on the
/// wall's `WALL` XDATA record so subsequent regenerations can erase them.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner(scene, wall_handle, None, None, library_override)
}

/// Like [`regenerate_wall_representation`], but honors per-slot visibility
/// from `rules` (see [`engine::display_component::ComponentRuleSet`]):
/// a `WALL_REP` child whose corresponding [`engine::display_component::WallComponentSlot`]
/// is hidden simply isn't created, instead of a global LOD switch. `None`
/// (or a default rule set) reproduces today's behavior exactly (every slot
/// defaults to visible).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        None,
        None,
        rules,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_rules`], but additionally
/// honors a `DisplayConfig`'s `style_substitutions` map (source wall style
/// id -> target wall style id, see [`engine::plan_view::DisplayConfig`]):
/// when the wall's own style id has an entry here, the *style* (material/
/// hatch/color) of each layer is taken from the corresponding layer (by
/// index) of the target wall style, while axis, thickness and layer count
/// are always derived from the wall's own (unchanged) layers. `None`
/// reproduces today's behavior exactly (no substitution applied).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        None,
        None,
        rules,
        style_substitutions,
        library_override,
    )
}

/// Rebuild a wall after its axis vertices changed (grip / stretch) and
/// re-resolve nearby L/T/N junctions. Returns every axis + derived handle
/// that the scene tessellation must refresh.
pub fn refresh_wall_after_axis_edit(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut touched = try_auto_join_nearby_walls(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !touched.iter().any(|h| *h == wall_handle) {
        match regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(wall_handle),
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    // Resident wire cache can keep the previous 2D outline if only
    // Added/Removed deltas are replayed after a delete+recreate. Force a
    // full rebuild so the contour on screen matches the new axis.
    scene.bump_geometry();
    touched
}

/// Move a wall's **axis** vertices (not its visible contour/hatch/solid
/// children) by `delta` wherever `in_win` reports the vertex as selected,
/// then regenerate + re-join via [`refresh_wall_after_axis_edit`].
///
/// The wall axis lives on the invisible `AEC_WALL_AXIS` layer, so commands
/// like STRETCH that hit-test only visible geometry never see it — they only
/// ever get a handle to the derived contour. Moving that derived contour's
/// own vertices instead of the axis is reverted by the very next
/// regeneration (which rebuilds the contour from the unchanged axis), so any
/// wall-aware caller must resolve to the axis and move *it* first. This
/// function is that shared operation; used by STRETCH in
/// `command_driver.rs`.
///
/// Returns `None` if `owner` isn't a wall, has no axis vertices, or none of
/// them fall inside the window (no-op). Otherwise returns every axis +
/// derived handle touched by the regeneration, exactly like
/// [`refresh_wall_after_axis_edit`].
pub fn stretch_wall_axis_in_window(
    scene: &mut Scene,
    owner: Handle,
    in_win: impl Fn(f64, f64) -> bool,
    delta: DVec3,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Option<Vec<Handle>> {
    let axis_vertices = get_wall_vertices(scene, owner);
    if axis_vertices.is_empty() {
        return None;
    }
    let mut moved = false;
    let new_vertices: Vec<DVec3> = axis_vertices
        .iter()
        .map(|v| {
            if in_win(v.x, v.y) {
                moved = true;
                DVec3::new(v.x + delta.x, v.y + delta.y, v.z + delta.z)
            } else {
                *v
            }
        })
        .collect();
    if !moved {
        return None;
    }
    update_wall_vertices(scene, owner, &new_vertices);
    Some(refresh_wall_after_axis_edit(
        scene,
        owner,
        library_override,
        display_rules,
        style_substitutions,
    ))
}

/// Like [`regenerate_wall_representation`], but lets a caller supply a
/// corner-extension hint: `(vertex_index, extended_position)` moves one axis
/// vertex further out — past a joined corner and into the other wall's
/// footprint — for contour/hatch/solid generation only. The persisted axis
/// polyline (and therefore `AEC_ROOM` loop detection, which depends on the
/// exact trimmed corner) is left untouched; only the *visible representation*
/// uses the extended point.
///
/// When `join_miter` is supplied, matched layers (by material/function then
/// offset-from-axis) are rebuilt with a true diagonal miter against the other
/// wall's corresponding layer; unmatched layers fall back to the single-vertex
/// `corner_override` extension.
///
/// Used by [`join_two_walls_in_document`] so an L/T join's two walls share a
/// clean mitered corner instead of leaving a seam where their
/// independently-capped rectangles merely touch.
///
/// On success returns the axis handle plus every newly created derived
/// (contour/hatch/solid) handle, so callers (e.g. grip-release) can bump 2D
/// and 3D representations together.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_and_rules(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but also honors
/// per-slot visibility from `rules` (see [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_corner_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner_and_rules`], but also
/// honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_corner_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        join_miter,
        None,
        rules,
        style_substitutions,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_corner`], but takes precomputed
/// per-layer miter footprints (from N-way junction resolution). `None` entries
/// fall back to `corner_override` / base contours exactly as unmatched layers do.
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_and_rules(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters`], but also
/// honors per-slot visibility from `rules` (see
/// [`regenerate_wall_representation_with_rules`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_and_rules(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
        scene,
        wall_handle,
        corner_override,
        mitered_footprints,
        rules,
        None,
        library_override,
    )
}

/// Like [`regenerate_wall_representation_with_precomputed_miters_and_rules`],
/// but also honors `style_substitutions` (see
/// [`regenerate_wall_representation_with_rules_and_substitutions`]).
/// `library_override`: when `Some`, used instead of the global on-disk library — pass the project-resolved library when a project is active.
pub fn regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    mitered_footprints: &[Option<Vec<(f64, f64)>>],
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    regenerate_wall_representation_inner(
        scene,
        wall_handle,
        corner_override,
        None,
        Some(mitered_footprints),
        rules,
        style_substitutions,
        library_override,
    )
}

/// Locate a wall's already-established join(s) — pairwise (L/T) or N-way —
/// at the axis end that is *not* `handled_end`, using the peer-link index
/// maintained by `engine::owner_index`, cross-referenced against which of
/// this wall's own axis ends is actually coincident with a peer's endpoint
/// or lies on a peer's span. Reuses the exact same junction-detection and
/// per-layer-miter machinery as `join_junction_in_document`
/// (`join::detect_junctions` + `engine::miter::junction_wall_geoms` +
/// `mitered_junction_layer_footprints_with_overrides`), but purely
/// read-only: it never mutates axis vertices or persists overrides for
/// participants other than confirming this wall's own existing
/// `JunctionOverride`.
///
/// Returns `None` when there is no peer at the other end (the common case:
/// a wall with only one join, or a freshly drawn unjoined end), which keeps
/// `regenerate_wall_representation_inner` byte-for-byte unchanged for that
/// case. Used so a NEW join event at one end doesn't silently drop an
/// already-established join at the other end during regeneration.
fn find_other_end_junction_footprints(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
    handled_end: usize,
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    // `handled_end` is the axis vertex we want footprints for (the caller
    // already skipped the end covered by this regen's primary join).
    let other_end = handled_end;
    if other_end >= self_axis_2d.len() {
        return None;
    }
    let pt = self_axis_2d[other_end];
    let pt3 = DVec3::new(pt.0, pt.1, 0.0);
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);

    // Participants: this wall plus every currently-linked peer whose axis is
    // actually coincident (endpoint or through-span) with this wall's other
    // end. `handles[0]` is always `wall_handle`.
    let mut handles = vec![wall_handle];
    let mut candidates = engine::owner_index::peers_of(&scene.document, wall_handle);
    for h in all_wall_axis_handles(scene) {
        if h != wall_handle && !candidates.contains(&h) {
            candidates.push(h);
        }
    }
    for peer in candidates {
        if peer == wall_handle || handles.contains(&peer) {
            continue;
        }
        let peer_axis = get_wall_vertices(scene, peer);
        if peer_axis.len() < 2 {
            continue;
        }
        let end_hit = peer_axis[0].distance(pt3) <= tol || peer_axis.last().unwrap().distance(pt3) <= tol;
        let through_hit = (0..peer_axis.len() - 1).any(|i| {
            point_to_segment_dist_2d(pt3, peer_axis[i], peer_axis[i + 1]) <= WALL_JOIN_SNAP_RADIUS
                && peer_axis[i].distance(pt3) > join::END_MID_TOLERANCE
                && peer_axis[i + 1].distance(pt3) > join::END_MID_TOLERANCE
        });
        if end_hit || through_hit {
            handles.push(peer);
        }
    }
    if handles.len() < 2 {
        return None;
    }

    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let junctions = join::detect_junctions(&axis_refs, tol);
    let junc = junctions.into_iter().find(|j| j.point.distance(pt3) <= tol.max(1e-3))?;

    // Confirm this wall (`handles[0]`) actually participates as an endpoint
    // at `other_end` in the detected junction (guards against picking up an
    // unrelated junction that happens to share the same point).
    let self_wall_index = 0usize;
    let pi = junc
        .participants
        .iter()
        .position(|p| p.wall_index == self_wall_index)?;
    if !matches!(junc.participants[pi].role, join::JunctionRole::Endpoint(e) if e == other_end) {
        return None;
    }

    let axes_2d: Vec<Vec<(f64, f64)>> = axes
        .iter()
        .map(|a| a.iter().map(|p| (p.x, p.y)).collect())
        .collect();
    let layers: Vec<Vec<engine::miter::MiterLayer>> =
        handles.iter().map(|h| wall_layer_data(scene, *h)).collect();
    let geoms = engine::miter::junction_wall_geoms(&junc, &axes_2d, &layers);
    let layer_refs: Vec<Vec<join::LayerRef>> = junc
        .participants
        .iter()
        .map(|p| {
            layers
                .get(p.wall_index)
                .map(|ls| {
                    layer_refs_from_materials(ls.iter().map(|l| (l.material.as_str(), l.layer_id)))
                })
                .unwrap_or_default()
        })
        .collect();
    let junction_overrides: Vec<Option<join::JunctionOverride>> = junc
        .participants
        .iter()
        .enumerate()
        .map(|(oi, p)| match p.role {
            join::JunctionRole::Endpoint(end_idx) => {
                read_junction_override(scene, handles[p.wall_index], end_idx).and_then(|ov| {
                    let self_refs = layer_refs.get(oi).cloned().unwrap_or_default();
                    let other_refs: Vec<join::LayerRef> = layer_refs
                        .iter()
                        .enumerate()
                        .filter(|(oi2, _)| *oi2 != oi)
                        .flat_map(|(_, ls)| ls.iter().cloned())
                        .collect();
                    validate_and_persist_junction_override(
                        scene,
                        handles[p.wall_index],
                        end_idx,
                        ov,
                        &self_refs,
                        &other_refs,
                    )
                })
            }
            join::JunctionRole::Through(_) => None,
        })
        .collect();
    let all_fps = engine::miter::mitered_junction_layer_footprints_with_overrides(
        &junc,
        &geoms,
        &layer_refs,
        &junction_overrides,
    );
    all_fps.get(pi).cloned()
}

/// T-junction cutouts when this wall is the through (head) wall: the join
/// sits on a span, not an axis end, so [`find_other_end_junction_footprints`]
/// never sees it. Without this, plan-type / 2D-3D regen drops pockets.
fn find_through_span_cutout_footprints(
    scene: &mut Scene,
    wall_handle: Handle,
    self_axis_2d: &[(f64, f64)],
) -> Option<Vec<Option<Vec<(f64, f64)>>>> {
    if self_axis_2d.len() < 2 {
        return None;
    }
    let handles = all_wall_axis_handles(scene);
    if handles.len() < 2 {
        return None;
    }
    let self_i = handles.iter().position(|h| *h == wall_handle)?;
    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    if axes.iter().any(|a| a.len() < 2) {
        return None;
    }
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions(&axis_refs, tol);
    let junc = junctions.into_iter().find(|j| {
        j.participants.iter().any(|p| {
            p.wall_index == self_i && matches!(p.role, join::JunctionRole::Through(_))
        }) && j
            .participants
            .iter()
            .any(|p| matches!(p.role, join::JunctionRole::Endpoint(_)))
    })?;
    let stem = junc
        .participants
        .iter()
        .find(|p| matches!(p.role, join::JunctionRole::Endpoint(_)))?;
    let join::JunctionRole::Endpoint(stem_end) = stem.role else {
        return None;
    };
    let stem_handle = handles[stem.wall_index];
    let stem_axis: Vec<(f64, f64)> = axes[stem.wall_index]
        .iter()
        .map(|p| (p.x, p.y))
        .collect();
    let through_layers = wall_layer_data(scene, wall_handle);
    let stem_layers = wall_layer_data(scene, stem_handle);
    Some(engine::miter::through_wall_cutout_footprints_with_bulges(
        self_axis_2d,
        &through_layers,
        &stem_axis,
        &stem_layers,
        stem_end,
        &get_wall_bulges(scene, wall_handle),
        &get_wall_bulges(scene, stem_handle),
    ))
}

fn regenerate_wall_representation_inner(
    scene: &mut Scene,
    wall_handle: Handle,
    corner_override: Option<(usize, DVec3)>,
    join_miter: Option<&engine::miter::JoinMiterContext>,
    precomputed_miters: Option<&[Option<Vec<(f64, f64)>>]>,
    rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, WallRegenError> {
    use engine::display_component::{LayerSelection, WallComponentSlot};
    // Geometry vs. hatch are independent: layer contours/hatches vs. overall
    // contour/hatch. Envelope (first/last layer) stands in for overall contour
    // until a dedicated union path exists. Axis/SurfaceStyle3D/Section/Elevation
    // remain no-ops here.
    let layers2d_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Layers2D));
    let contour2d_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Contour2D));
    let layer_hatch_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::LayerHatch2D));
    let contour_hatch_visible =
        rules.map_or(true, |r| r.is_visible(WallComponentSlot::ContourHatch2D));
    let solid_visible = rules.map_or(true, |r| r.is_visible(WallComponentSlot::Solid3D));
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return Err(WallRegenError::NotAWall);
    };

    let Some(wall) = wall_from_entity(entity) else {
        return Err(WallRegenError::NotAWall);
    };
    let (layers, height, _old_derived, wall_style_id, wall_hatch_override) = (
        wall.layers,
        wall.height,
        wall.derived_handles,
        wall.style_id,
        wall.hatch_override,
    );
    let wall_hatch_override = wall_hatch_override.as_ref();

    if layers.is_empty() {
        return Err(WallRegenError::NoLayers);
    }

    // Keep existing contour polylines so their tessellation handles stay
    // valid (delete+recreate left the old outline on screen). Hatches and
    // solids are still replaced; leftover contours are erased after reuse.
    let stale = collect_wall_display_children(scene, wall_handle);
    let mut reusable_contours: Vec<Handle> = Vec::new();
    let mut erase_now: Vec<Handle> = Vec::new();
    for h in stale {
        match scene.document.get_entity(h) {
            Some(EntityType::LwPolyline(_)) => reusable_contours.push(h),
            _ => erase_now.push(h),
        }
    }
    if !erase_now.is_empty() {
        scene.erase_entities(&erase_now);
    }

    // The axis is reference geometry on its own layer. Idle visibility follows
    // the AxisLine slot when rules are present; otherwise the layer stays off.
    set_wall_axis_layer_visible(scene, axis_visible_from_rules(rules));
    if let Some(e) = scene.document.get_entity_mut(wall_handle) {
        e.as_entity_mut().set_layer(AEC_WALL_AXIS_LAYER.to_string());
    }

    let axis_entity = scene
        .document
        .get_entity(wall_handle)
        .cloned()
        .ok_or(WallRegenError::NotAWall)?;

    // Base footprints from the true (persisted) axis via the shared
    // WallRepresentation builder. Corner extension is only applied as a
    // per-layer fallback when a join miter can't match layers.
    let base_footprints_with_bulges = wall_layer_footprints_with_bulges(&axis_entity, &layers);
    if base_footprints_with_bulges.is_empty() {
        if !reusable_contours.is_empty() {
            scene.erase_entities(&reusable_contours);
        }
        let _ = set_wall_derived_handles(scene, wall_handle, &[]);
        return Err(WallRegenError::NoContours);
    }
    let base_footprints: Vec<Vec<(f64, f64)>> = base_footprints_with_bulges
        .iter()
        .map(|(pts, _)| pts.clone())
        .collect();
    let base_footprint_bulges: Vec<Vec<f64>> = base_footprints_with_bulges
        .iter()
        .map(|(_, b)| b.clone())
        .collect();

    // Openings hosted by this wall — used to split 2D contours/hatches into
    // disconnected pieces. 3D solids stay uncut in this step (deferred).
    let wall_openings = openings_for_host_wall(scene, wall_handle);
    let (centerline, bulges) = wall_axis_points_and_bulges(&axis_entity);
    let layer_data: Vec<(f64, f64)> =
        layers.iter().map(|l| (l.thickness, l.axis_offset)).collect();
    let layer_extrusion: Vec<(f64, f64)> = layers
        .iter()
        .map(|l| {
            (
                (height - l.bottom_offset - l.top_offset).max(0.0),
                l.bottom_offset,
            )
        })
        .collect();
    let display = engine::representation::build_wall_display_set(
        &centerline,
        &bulges,
        &layer_data,
        0.0,
        &wall_openings,
        &layer_extrusion,
    );
    let opening_cut_layers: Option<Vec<Vec<Vec<(f64, f64)>>>> =
        if display.rep2d.cut_layer_pieces_2d.is_empty() {
            None
        } else {
            Some(display.rep2d.cut_layer_pieces_2d.clone())
        };

    // Fallback footprints: axis with a single vertex pushed past the join into
    // the other wall's footprint (legacy corner-overlap path).
    let mut extended_axis_entity = axis_entity.clone();
    let has_corner_override = if let (Some((idx, pos)), EntityType::LwPolyline(pl)) =
        (corner_override, &mut extended_axis_entity)
    {
        if let Some(v) = pl.vertices.get_mut(idx) {
            v.location = Vector2::new(pos.x, pos.y);
            true
        } else {
            false
        }
    } else {
        false
    };
    let extended_footprints = if has_corner_override {
        wall_layer_footprints(&extended_axis_entity, &layers)
    } else {
        Vec::new()
    };

    // Self axis as plain 2D points for the miter helper.
    let self_axis_2d: Vec<(f64, f64)> = match &axis_entity {
        EntityType::LwPolyline(pl) => pl
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect(),
        _ => Vec::new(),
    };
    let self_layer_data: Vec<engine::miter::MiterLayer> = layers
        .iter()
        .map(|l| {
            engine::miter::MiterLayer::with_id(
                l.thickness,
                l.axis_offset,
                l.material.clone(),
                l.function.clone(),
                l.layer_id,
            )
        })
        .collect();

    // Shared helper: build per-layer mitered footprints for one end's join
    // context, honoring any persisted `JunctionOverride` for that end.
    let compute_end_footprints = |scene: &mut Scene, ctx: &engine::miter::JoinMiterContext| {
        if ctx.as_through {
            let stem_end = ctx.other_end.unwrap_or(ctx.self_end);
            return engine::miter::through_wall_cutout_footprints_with_bulges(
                &self_axis_2d,
                &self_layer_data,
                &ctx.other_axis,
                &ctx.other_layers,
                stem_end,
                &ctx.self_bulges,
                &ctx.other_bulges,
            );
        }
        let self_layer_refs: Vec<join::LayerRef> =
            layer_refs_from_materials(layers.iter().map(|l| (l.material.as_str(), l.layer_id)));
        let junction_override = read_junction_override(scene, wall_handle, ctx.self_end)
            .and_then(|ov| {
                let other_layer_refs: Vec<join::LayerRef> = layer_refs_from_materials(
                    ctx.other_layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                );
                validate_and_persist_junction_override(
                    scene,
                    wall_handle,
                    ctx.self_end,
                    ov,
                    &self_layer_refs,
                    &other_layer_refs,
                )
            });
        engine::miter::mitered_layer_footprints_with_override_and_bulges(
            &self_axis_2d,
            &self_layer_data,
            &self_layer_refs,
            ctx.self_end,
            &ctx.other_axis,
            &ctx.other_layers,
            ctx.other_end,
            ctx.kind,
            junction_override.as_ref(),
            &ctx.self_bulges,
            &ctx.other_bulges,
        )
    };

    // Pre-compute per-layer mitered footprints when a join context is present,
    // or use caller-supplied N-way junction footprints.
    let mut mitered_footprints: Vec<Option<Vec<(f64, f64)>>> =
        if let Some(pre) = precomputed_miters {
            let mut v = pre.to_vec();
            v.resize(layers.len(), None);
            v
        } else if let Some(ctx) = join_miter {
            compute_end_footprints(scene, ctx)
        } else {
            vec![None; layers.len()]
        };

    // Whichever end this call's `join_miter` handled (if any) shouldn't be
    // re-derived below; every other axis end that currently has an
    // established peer join must also be reflected here, or that end's
    // rendering would revert to a plain unjoined cap whenever this wall is
    // regenerated for a *different* join event (see module-level bug notes
    // on `find_other_end_join_miter`).
    if self_axis_2d.len() >= 2 {
        let mut candidate_ends = vec![0usize, self_axis_2d.len() - 1];
        candidate_ends.dedup();
        if let Some(ctx) = join_miter {
            candidate_ends.retain(|&e| e != ctx.self_end);
        }
        for end_idx in candidate_ends {
            if let Some(other_footprints) =
                find_other_end_junction_footprints(scene, wall_handle, &self_axis_2d, end_idx)
            {
                for (i, base_fp) in base_footprints.iter().enumerate() {
                    let merged = engine::miter::merge_end_footprints(
                        base_fp,
                        mitered_footprints.get(i).and_then(|o| o.as_ref()),
                        other_footprints.get(i).and_then(|o| o.as_ref()),
                    );
                    if i < mitered_footprints.len() {
                        mitered_footprints[i] = merged;
                    }
                }
            }
        }
        if !join_miter.map(|c| c.as_through).unwrap_or(false) {
            if let Some(cut) =
                find_through_span_cutout_footprints(scene, wall_handle, &self_axis_2d)
            {
                for (i, fp) in cut.into_iter().enumerate() {
                    if fp.is_some() && i < mitered_footprints.len() {
                        mitered_footprints[i] = fp;
                    }
                }
            }
        }
    }

    // Extrusion height/base come from the (possibly extended) axis so solids
    // stay consistent with the 2D footprint chosen per layer below.
    let extrusion_axis = if has_corner_override {
        &extended_axis_entity
    } else {
        &axis_entity
    };
    let extrusions = wall_layer_extrusions(extrusion_axis, &layers, height);
    // Prefer a caller-supplied (usually project-resolved) library so hatch
    // scale/pattern/color and material lookups match the active project
    // instead of always reading the global on-disk defaults.
    let owned_library;
    let library: &StyleLibrary = match library_override {
        Some(lib) => lib,
        None => {
            owned_library = load_or_seed();
            &owned_library
        }
    };

    // `StyleSubstitution`: when the wall's own style has a target entry,
    // the target wall style's layers become the *style* source (material,
    // and therefore hatch/color) for the corresponding layer index, while
    // axis/thickness/layer count always stay derived from `layers` above —
    // only the style-lookup material id per layer changes. Layer count
    // mismatches (fewer target layers than source) simply leave the
    // remaining layers on their own original material (no substitution for
    // those indices), matching the plan's precedence chain (c falls back
    // to (d)/(e) for anything the substitution can't resolve).
    let substituted_layer_styles: Option<Vec<(String, Option<String>)>> =
        style_substitutions.and_then(|subs| {
            subs.get(&wall_style_id).and_then(|target_id| {
                library
                    .wall_styles
                    .iter()
                    .find(|s| &s.style.id == target_id)
                    .map(|s| {
                        s.layers
                            .iter()
                            .map(|l| (l.material_id.clone(), l.hatch_override.clone()))
                            .collect()
                    })
            })
        });

    // Overall wall run direction (radians), used as the base angle for
    // materials whose hatch angle is relative to the wall instead of a
    // fixed/global angle.
    let wall_angle_rad = {
        let first = centerline.first().copied();
        let last = centerline.last().copied();
        match (first, last) {
            (Some((x0, y0)), Some((x1, y1))) if (x1 - x0).abs() > 1e-9 || (y1 - y0).abs() > 1e-9 => {
                (y1 - y0).atan2(x1 - x0)
            }
            _ => 0.0,
        }
    };

    let mut new_derived: Vec<Handle> = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let mat_name = &layer.material;
        // Prefer a true per-layer miter when the join helper could match this
        // layer index against the other wall; otherwise fall back to the
        // corner-extended footprint (or the plain base footprint) from
        // WallRepresentation.
        // Prefer a true per-layer miter / corner-extended footprint when
        // available (those paths are still straight-only). Otherwise use the
        // bulge-aware base footprint so curved axes keep exact offset arcs.
        // With openings and no miter/corner override, emit one 2D contour+hatch
        // per disconnected piece (through-cut splits the band).
        let uncut_footprint: (Vec<(f64, f64)>, Vec<f64>) =
            if let Some(Some(mitered)) = mitered_footprints.get(i) {
                let bg = retarget_closed_footprint_bulges(
                    &base_footprints[i],
                    base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                    mitered,
                );
                (mitered.clone(), bg)
            } else if let Some(fp) = extended_footprints.get(i) {
                let bg = retarget_closed_footprint_bulges(
                    &base_footprints[i],
                    base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                    fp,
                );
                (fp.clone(), bg)
            } else {
                (
                    base_footprints[i].clone(),
                    base_footprint_bulges
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; base_footprints[i].len()]),
                )
            };

        let use_opening_cuts = mitered_footprints.get(i).and_then(|o| o.as_ref()).is_none()
            && extended_footprints.get(i).is_none()
            && opening_cut_layers.is_some();

        let pieces_2d: Vec<(Vec<(f64, f64)>, Vec<f64>)> = if use_opening_cuts {
            opening_cut_layers
                .as_ref()
                .and_then(|cuts| cuts.get(i))
                .map(|pieces| {
                    pieces
                        .iter()
                        .map(|p| (p.clone(), vec![0.0; p.len()]))
                        .collect()
                })
                .unwrap_or_else(|| vec![uncut_footprint.clone()])
        } else {
            let rings = engine::miter::split_footprint_rings(&uncut_footprint.0);
            if rings.len() > 1 {
                rings
                    .into_iter()
                    .map(|r| {
                        let n = r.len();
                        (r, vec![0.0; n])
                    })
                    .collect()
            } else {
                vec![uncut_footprint.clone()]
            }
        };

        // Stable per-layer identity, used to match `layer_filter` /
        // `layer_style_override` entries against this layer (mirrors
        // `layer_refs_from_materials`'s convention: `WallLayer` carries no
        // `role_tag`, so it's always `None` here).
        let layer_ref = join::LayerRef {
            material_id: mat_name.clone(),
            role_tag: None,
            index: i,
            layer_id: Some(layer.layer_id),
        };
        // `layer_filter_for(slot)` gates `Contour2D`/`Solid3D` independently
        // (Step 2/3): the 2D contour below consults the `Contour2D` slot,
        // while the 3D solid further down consults `Solid3D` separately —
        // so e.g. a 5-layer wall can show only the masonry layer as its 2D
        // contour while still extruding every layer's solid. `All` (or no
        // rules) keeps every layer, exactly like before this feature
        // existed.
        let layer_included_contour = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Contour2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_layers2d = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Layers2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_hatch = match rules.map(|r| r.layer_filter_for(WallComponentSlot::LayerHatch2D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };
        let layer_included_solid = match rules.map(|r| r.layer_filter_for(WallComponentSlot::Solid3D)) {
            Some(LayerSelection::Explicit(refs)) => layer_ref_matches(&layer_ref, refs),
            _ => true,
        };

        // `StyleSubstitution` (precedence tier c): swap the *style* source
        // (material id + its own hatch override) for this layer index from
        // the resolved target wall style, while `layer`'s own geometry
        // (thickness/gaps/offsets) is untouched. Falls back to this layer's
        // own material/hatch_override when the target has no layer at this
        // index (or no substitution applies at all).
        let (effective_mat_name, effective_hatch_override): (&str, Option<&str>) =
            match substituted_layer_styles.as_ref().and_then(|v| v.get(i)) {
                Some((sub_mat, sub_hatch)) => (sub_mat.as_str(), sub_hatch.as_deref()),
                None => (mat_name.as_str(), layer.hatch_override.as_deref()),
            };
        let material = library.materials.iter().find(|m| {
            m.id == effective_mat_name || m.name == effective_mat_name
        });

        // Envelope (overall 2D contour) prefers `Contour2D` so phase extras
        // (Abbruch/Bestand) win over per-layer `Layers2D` overrides.
        let is_envelope = i == 0 || i + 1 == layers.len();
        let contour_slots = if is_envelope {
            [WallComponentSlot::Contour2D, WallComponentSlot::Layers2D]
        } else {
            [WallComponentSlot::Layers2D, WallComponentSlot::Contour2D]
        };
        let contour_style_override =
            resolve_layer_style_override(rules, &contour_slots, &layer_ref);
        let hatch_slots = if is_envelope {
            [
                WallComponentSlot::ContourHatch2D,
                WallComponentSlot::LayerHatch2D,
            ]
        } else {
            [
                WallComponentSlot::LayerHatch2D,
                WallComponentSlot::ContourHatch2D,
            ]
        };
        let mut hatch_style_override =
            resolve_layer_style_override(rules, &hatch_slots, &layer_ref);
        if is_envelope {
            if let Some(slot_style) = rules.and_then(|r| {
                r.style_for(WallComponentSlot::ContourHatch2D)
            }) {
                hatch_style_override.overlay_from(slot_style);
            }
        }

        // Layer-level `hatch_override` (Step 4) takes precedence over the
        // material's own `hatch_pattern`; both fall back to "ANSI31" so a
        // layer/material without an explicit pattern still renders a hatch.
        let pattern_name = hatch_style_override
            .hatch_pattern
            .clone()
            .or_else(|| {
                effective_hatch_override
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string())
            })
            .or_else(|| material.map(|m| m.hatch_pattern.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_else(|| "ANSI31".to_string());
        let hatch_acad_color = hatch_style_override
            .hatch_color
            .or_else(|| material.and_then(|m| m.hatch_color))
            .or_else(|| material.map(|m| material_line_color_as_acad(m.line_color)));
        let color = hatch_acad_color
            .map(wall_hatch_color)
            .unwrap_or([0.6, 0.6, 0.6, 0.85]);
        let mut hatch_scale = hatch_style_override
            .hatch_scale
            .or_else(|| material.map(|m| m.hatch_scale))
            .unwrap_or(1.0);
        if hatch_scale <= 0.0 {
            hatch_scale = 0.01;
        }
        let hatch_scale = hatch_scale as f32;
        // Hatch direction: either the material's own hatch angle applied on
        // top of the wall's run direction ("relative"), or used verbatim as
        // a fixed/global angle. Both are stored in degrees on `Material` and
        // converted to the radians `HatchModel::angle_offset` expects.
        // Override precedence: `Wall.hatch_override` (per-instance) >
        // style-profile `hatch_style_override` > `Material` default.
        let hatch_angle_deg = wall_hatch_override
            .and_then(|ov| ov.hatch_angle)
            .or(hatch_style_override.hatch_angle)
            .or_else(|| material.map(|m| m.hatch_angle))
            .unwrap_or(0.0);
        let hatch_angle_relative = wall_hatch_override
            .and_then(|ov| ov.hatch_angle_relative)
            .or(hatch_style_override.hatch_angle_relative)
            .or_else(|| material.map(|m| m.hatch_angle_relative))
            .unwrap_or(true);
        let hatch_angle_offset = if hatch_angle_relative {
            wall_angle_rad + hatch_angle_deg.to_radians()
        } else {
            hatch_angle_deg.to_radians()
        } as f32;
        let line_color = contour_style_override
            .line_color
            .or_else(|| material.map(|m| material_line_color_as_acad(m.line_color)));
        let fill_color = contour_style_override.fill_color;
        let families = crate::scene::model::hatch_patterns::find(&pattern_name)
            .and_then(|e| {
                if let crate::scene::model::hatch_model::HatchPattern::Pattern(f) = &e.gpu {
                    Some(f.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // 2D contour + hatch for each remaining piece after openings.
        for (footprint, footprint_bulges) in &pieces_2d {
            if footprint.len() < 3 {
                continue;
            }

            let draw_contour = (layers2d_visible && layer_included_layers2d)
                || (contour2d_visible && is_envelope && layer_included_contour);
            let draw_hatch = (layer_hatch_visible && layer_included_hatch)
                || (contour_hatch_visible && is_envelope);

            if draw_contour {
                let mut pl = LwPolyline::new();
                for (idx, &(x, y)) in footprint.iter().enumerate() {
                    let bulge = footprint_bulges.get(idx).copied().unwrap_or(0.0);
                    pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                }
                pl.is_closed = true;
                let contour_handle =
                    reuse_or_add_wall_contour(scene, &mut reusable_contours, pl);
                if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
                    scene.ensure_layer(layer_name);
                    if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                        e.as_entity_mut().set_layer(layer_name.to_string());
                    }
                }
                if let Some(color) = line_color {
                    if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                        e.as_entity_mut().set_color(color);
                    }
                }
                if let Some(lt) = contour_style_override.line_type.as_deref().filter(|s| !s.is_empty()) {
                    if let Some(e) = scene.document.get_entity_mut(contour_handle) {
                        e.common_mut().linetype = lt.to_string();
                    }
                }
                write_wall_display_tag(scene, contour_handle, wall_handle, WALL_REP_ROLE_CONTOUR);
                new_derived.push(contour_handle);
            }

            if draw_hatch {
                let tessellated = tessellate_ring_with_bulges(footprint, footprint_bulges);
                let (rel, origin, wcs) = pack_wall_ring(&tessellated);
                let hatch_model = crate::scene::model::hatch_model::HatchModel {
                    render_instance: None,
                    boundary: std::sync::Arc::new(rel),
                    pattern: crate::scene::model::hatch_model::HatchPattern::Pattern(families.clone()),
                    name: pattern_name.clone(),
                    color,
                    aci: 0,
                    line_weight_px: 1.0,
                    angle_offset: hatch_angle_offset,
                    scale: hatch_scale,
                    world_origin: origin,
                    boundary_wcs: Some(std::sync::Arc::new(wcs)),
                    fill_plane: None,
                    fill_plane_boundary: None,
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: acadrust::entities::HatchStyleType::Normal,
                    draw_depth: 0.0,
                };
                let hatch_style = hatch_acad_color.map(|c| {
                    (c, acadrust::types::Transparency::from_percent(0.0))
                });
                let hatch_handle = scene.add_hatch(hatch_model, None, hatch_style);
                if let Some(layer_name) = layer.layer_override.as_deref().filter(|s| !s.is_empty()) {
                    scene.ensure_layer(layer_name);
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_layer(layer_name.to_string());
                    }
                }
                if let Some(c) = hatch_acad_color {
                    if let Some(e) = scene.document.get_entity_mut(hatch_handle) {
                        e.as_entity_mut().set_color(c);
                    }
                }
                write_wall_display_tag(scene, hatch_handle, wall_handle, WALL_REP_ROLE_HATCH);
                new_derived.push(hatch_handle);
            }
        }

        // Extruded solid for this layer — same rings as 2D (miter/cutout/
        // corner override, else WallDisplaySet solids). 3D opening boolean
        // remains deferred.
        let (solid_height, solid_base) = if let Some(ex) = extrusions.get(i) {
            (ex.height, ex.base_offset)
        } else if let Some(solid) = display.solids.get(i) {
            (solid.height, solid.base_offset)
        } else {
            (height, 0.0)
        };
        let solid_rings: Vec<(Vec<(f64, f64)>, Vec<f64>)> = if !pieces_2d.is_empty()
            && mitered_footprints.get(i).and_then(|o| o.as_ref()).is_some()
        {
            pieces_2d.clone()
        } else if let Some(fp) = extended_footprints.get(i) {
            let bg = retarget_closed_footprint_bulges(
                &base_footprints[i],
                base_footprint_bulges.get(i).map(|b| b.as_slice()).unwrap_or(&[]),
                fp,
            );
            vec![(fp.clone(), bg)]
        } else if let Some(solid) = display.solids.get(i) {
            vec![(solid.footprint.clone(), solid.bulges.clone())]
        } else {
            vec![uncut_footprint.clone()]
        };
        if solid_visible && layer_included_solid && solid_height.abs() > 1e-9 {
            for (footprint, footprint_bulges) in &solid_rings {
                if footprint.len() < 3 {
                    continue;
                }
                let mut pl = LwPolyline::new();
                for (idx, &(x, y)) in footprint.iter().enumerate() {
                    let bulge = footprint_bulges.get(idx).copied().unwrap_or(0.0);
                    pl.add_vertex(LwVertex::with_bulge(Vector2::new(x, y), bulge));
                }
                pl.is_closed = true;
                let contour_entity = EntityType::LwPolyline(pl);

                let to_extrude = if solid_base.abs() > 1e-9 {
                    let mut clone = contour_entity.clone();
                    if let EntityType::LwPolyline(ref mut pl) = clone {
                        pl.elevation = solid_base;
                    }
                    Some(clone)
                } else {
                    None
                };
                let entity_to_use = to_extrude.as_ref().unwrap_or(&contour_entity);

                if let Some(body) =
                    crate::scene::model::sweep_model::extruded(entity_to_use, solid_height)
                {
                    let mut s3d = acadrust::entities::Solid3D::new();
                    s3d.wires = crate::scene::model::solid_model::edge_wires(&body);
                    let solid_handle = scene.add_entity(EntityType::Solid3D(s3d));
                    scene.register_solid_model(solid_handle, body);
                    if let Some(color) = fill_color {
                        if let Some(e) = scene.document.get_entity_mut(solid_handle) {
                            e.as_entity_mut().set_color(color);
                        }
                    }
                    write_wall_display_tag(scene, solid_handle, wall_handle, WALL_REP_ROLE_SOLID);
                    new_derived.push(solid_handle);
                }
            }
        }
    }

    if !reusable_contours.is_empty() {
        scene.erase_entities(&reusable_contours);
    }

    let _ = set_wall_derived_handles(scene, wall_handle, &new_derived);
    // Keep storey membership index in sync whenever a wall is (re)built.
    register_wall_in_storey(scene, wall_handle);
    let mut touched = Vec::with_capacity(1 + new_derived.len());
    touched.push(wall_handle);
    touched.extend(new_derived.iter().copied());
    Ok(touched)
}

/// Rewrite the `derived_handles` tail of `wall_handle`'s `WALL` record,
/// keeping every other field unchanged. No-op (returns `false`) if the
/// entity doesn't carry a `WALL` record.
pub fn set_wall_derived_handles(
    scene: &mut Scene,
    wall_handle: Handle,
    derived_handles: &[Handle],
) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_from_entity(entity) else {
        return false;
    };
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        &v2.style_id,
        v2.height,
        v2.storey_id,
        &v2.layers,
        derived_handles,
        v2.justification, v2.phase, v2.hatch_override.as_ref());
    write_aec_record(&mut scene.document, wall_handle, record)
}

/// Changes an existing `WALL` wall's justification (Interior/Center/
/// Exterior), shifting its axis polyline sideways by the delta between the
/// old and new justification offsets (same `WallJustification::offset` math
/// used by [`WallCommand::build_entity`]), then regenerates its
/// contour/hatch/solid representation. No-op (returns `false`) if
/// `wall_handle` doesn't carry a `WALL` record.
pub fn change_wall_justification(
    scene: &mut Scene,
    wall_handle: Handle,
    new_justification: WallJustification,
    library_override: Option<&StyleLibrary>,
) -> bool {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(v2) = wall_from_entity(entity) else {
        return false;
    };

    let total_thickness = v2.total_thickness();
    let old_offset = v2.justification.offset(total_thickness);
    let new_offset = new_justification.offset(total_thickness);
    let delta = new_offset - old_offset;

    if delta.abs() > 1e-9 {
        let vertices = get_wall_vertices(scene, wall_handle);
        if vertices.len() >= 2 {
            let points: Vec<(f64, f64)> = vertices.iter().map(|v| (v.x, v.y)).collect();
            let directions = engine::get_offset_directions(&points);
            let shifted: Vec<DVec3> = points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| DVec3::new(x + dx * delta, y + dy * delta, 0.0))
                .collect();
            update_wall_vertices(scene, wall_handle, &shifted);
        }
    }

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = wall_record(
        &v2.style_id,
        v2.height,
        v2.storey_id,
        &v2.layers,
        &v2.derived_handles,
        new_justification, v2.phase, v2.hatch_override.as_ref());
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }

    let _ = regenerate_wall_representation(scene, wall_handle, library_override);
    true
}

/// Register the `OPENCAD_AEC` APPID up front so an interactive `AEC_WALL`
/// command can embed XDATA directly on entities it builds (it has no
/// `&mut CadDocument` while collecting points).
pub fn ensure_wall_app_id(doc: &mut CadDocument) {
    ensure_app_id(doc);
}

/// Default wall height (metres) offered by the command-line prompt after
/// the point chain is finished.
const DEFAULT_WALL_HEIGHT: f64 = 2.8;
/// Default wall thickness (metres) offered by the command-line prompt after
/// the height has been entered.
const DEFAULT_WALL_THICKNESS: f64 = 0.2;
const WALL_JOIN_SNAP_RADIUS: f64 = 0.3;

// The command used to walk through `WallPhase::AskStyle` / `AskHeight` /
// `AskThickness` command-line follow-up prompts once the point chain was
// finished. Style/height/justification are now always live-editable in the
// Properties panel while drawing (see `live_properties`/`apply_live_property`
// below), so the point chain finishes immediately (`start_dimension_prompt`)
// with whatever is currently set — no separate phase state machine needed.

/// `AEC_WALL` — interactive wall drawing. Each completed 2-point segment is
/// written immediately as its own `LwPolyline` + `WALL` XDATA; the command
/// stays active with the last end as the next start. Style/height/justification
/// stay live-editable in the Properties panel for the whole chain.
pub struct WallCommand {
    vertices: Vec<DVec3>,
    live_handle: Option<Handle>,
    live_contour_handle: Option<Handle>,
    plane: WorkingPlane,
    /// Parametric wall metadata written on each segment commit.
    wall: Wall,
    /// Fallback single-layer thickness when no style is selected.
    thickness: f64,
    library: Option<StyleLibrary>,
    style_id: Option<String>,
    resolved_layers: Option<Vec<WallLayer>>,
    justification: WallJustification,
    ctrl_was_down: bool,
    /// Set once the wall height was explicitly edited via the live
    /// Properties-panel field while drawing.
    height_live_set: bool,
    /// Set when the user tried to finish the wall (Enter/Escape) while a
    /// style selection was mandatory but not made yet; `prompt()` shows a
    /// hint until a style is picked.
    no_style_warning: bool,
    /// Per-segment LWPOLYLINE-style bulge (`bulges[i]` is the bulge of the
    /// segment from `vertices[i]` to `vertices[i + 1]`); parallel to
    /// `vertices`, trailing entry unused. `0.0` = straight segment.
    bulges: Vec<f64>,
    /// When set, the next placed point closes an arc segment (tangent-
    /// continuous with the previous segment) instead of a straight line;
    /// toggled on/off with the `A`/`L` command-line keywords, like `PLINE`.
    arc_mode: bool,
    /// Handle of the last persisted segment in this chain (for auto-join).
    last_committed: Option<Handle>,
    /// Start of the last committed axis, used as tangent reference for arc mode.
    last_axis_from: Option<DVec3>,
    /// Exit tangent of the last committed segment (plane-local), same as PLINE.
    last_tangent: Option<DVec2>,
    /// Number of segments written this command (Enter after ≥1 ends the chain).
    segments_committed: usize,
    /// Untagged live-preview contour handles to erase after the axis commit.
    pending_preview_companions: Vec<Handle>,
}

impl WallCommand {
    pub fn new() -> Self {
        // Always load a usable library: `load_or_seed` transparently creates
        // a small default library (materials + wall styles) on first use so
        // the style-selection prompt has something to offer without
        // requiring the user to define materials/styles first.
        Self::new_with_library(Some(engine::library::load_or_seed()))
    }

    pub fn new_with_library(library: Option<StyleLibrary>) -> Self {
        let mut cmd = Self {
            vertices: Vec::new(),
            live_handle: None,
            live_contour_handle: None,
            plane: WorkingPlane::default(),
            wall: Wall::new(String::new(), DEFAULT_WALL_HEIGHT, 0),
            thickness: DEFAULT_WALL_THICKNESS,
            library,
            style_id: None,
            resolved_layers: None,
            justification: WallJustification::Center,
            ctrl_was_down: false,
            height_live_set: false,
            no_style_warning: false,
            bulges: Vec::new(),
            arc_mode: false,
            last_committed: None,
            last_axis_from: None,
            last_tangent: None,
            segments_committed: 0,
            pending_preview_companions: Vec::new(),
        };
        cmd.apply_first_available_style();
        cmd
    }

    /// Pick the first library wall style so segments get a real representation
    /// even if the user never opens the style picker.
    fn apply_first_available_style(&mut self) {
        if self.style_id.is_some() {
            return;
        }
        let Some(lib) = &self.library else {
            return;
        };
        let Some(style) = lib.wall_styles.first() else {
            return;
        };
        let id = style.style.id.clone();
        if let Some(resolved) = resolve_wall_style_layers(lib, &id, None) {
            self.style_id = Some(id);
            self.resolved_layers = Some(resolved);
        }
    }

    /// Like [`Self::new`], but pre-fills the style/height with the given
    /// session defaults (typically the values used by the last wall
    /// finished this session) instead of the hardcoded fallback defaults.
    pub fn new_with_defaults(last_style_id: Option<&str>, last_height: Option<f64>) -> Self {
        Self::new().with_session_defaults(last_style_id, last_height)
    }

    /// Apply last-used style/height against the command's current library.
    /// Call this *after* [`Self::with_library`] so the id is resolved in the
    /// project library, not the seed library used by [`Self::new`].
    pub fn with_session_defaults(
        mut self,
        last_style_id: Option<&str>,
        last_height: Option<f64>,
    ) -> Self {
        if let Some(h) = last_height {
            self.wall.height = h;
            self.height_live_set = true;
        }
        if let Some(id) = last_style_id {
            if let Some(lib) = &self.library {
                if let Some(style) = lib.wall_styles.iter().find(|s| s.style.id == id) {
                    if let Some(resolved) =
                        resolve_wall_style_layers(lib, &style.style.id, None)
                    {
                        self.style_id = Some(style.style.id.clone());
                        self.resolved_layers = Some(resolved);
                    }
                }
            }
        }
        self
    }

    /// Replace the style library (e.g. project-resolved) and re-apply the
    /// current style id, or the first available style when the previous id
    /// is missing from the new library.
    pub fn with_library(mut self, library: StyleLibrary) -> Self {
        let previous = self.style_id.clone();
        self.library = Some(library);
        self.style_id = None;
        self.resolved_layers = None;
        if let Some(id) = previous {
            if let Some(lib) = &self.library {
                if let Some(style) = lib.wall_styles.iter().find(|s| s.style.id == id) {
                    if let Some(resolved) =
                        resolve_wall_style_layers(lib, &style.style.id, None)
                    {
                        self.style_id = Some(style.style.id.clone());
                        self.resolved_layers = Some(resolved);
                    }
                }
            }
        }
        if self.style_id.is_none() {
            self.apply_first_available_style();
        }
        self
    }

    /// Parse a command-line value, falling back to `default` for an empty
    /// input; rejects non-positive/invalid input by keeping the default.
    fn parse_dimension(text: &str, default: f64) -> f64 {
        let t = text.trim();
        if t.is_empty() {
            return default;
        }
        match t.parse::<f64>() {
            Ok(v) if v > 0.0 => v,
            _ => default,
        }
    }

    /// True once a style library with at least one wall style is loaded, in
    /// which case a wall style selection is mandatory before finishing (there
    /// is something to choose, so silently falling back to a styleless V1
    /// wall would be surprising). No library / an empty library means there
    /// is nothing to pick, so the plain height/thickness V1 wall stays valid.
    fn requires_style_selection(&self) -> bool {
        self.library
            .as_ref()
            .is_some_and(|lib| !lib.wall_styles.is_empty())
    }

    /// Begin prompting for the wall's height/thickness once the point chain
    /// is done; returns the result that keeps the command active for the
    /// command-line follow-up.
    fn start_dimension_prompt(&mut self) -> CmdResult {
        // Enter/Escape are global "finalize" keys in this app and fire even
        // while the user is typing in the live Properties-panel height field
        // (see `sync_live_if_previewable`) before a second point has been
        // placed. There's nothing to finalize yet in that case — keep the
        // command running instead of cancelling the whole wall.
        if self.vertices.len() < 2 {
            // A lone start point produces nothing. After at least one
            // committed segment, Enter/Escape ends the chain.
            if self.segments_committed > 0 {
                return CmdResult::Cancel;
            }
            return CmdResult::NeedPoint;
        }
        self.commit_current_segment()
    }

    fn commit_current_segment(&mut self) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        let start = self.vertices[0];
        let end = *self.vertices.last().unwrap();
        if start.distance(end) < 1e-9 {
            self.vertices.pop();
            self.bulges.pop();
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.build_entity() else {
            return CmdResult::NeedPoint;
        };
        self.last_axis_from = Some(start);
        let start_l = self.plane.to_local(start);
        let end_l = self.plane.to_local(end);
        let bulge = self.bulges.first().copied().unwrap_or(0.0);
        self.last_tangent = crate::modules::draw::draw::polyline::seg_exit_tangent(
            start_l,
            end_l,
            bulge,
        )
        .map(|t| t.as_dvec2());
        self.vertices = vec![end];
        self.bulges = vec![0.0];
        self.segments_committed += 1;
        self.pending_preview_companions = [self.live_handle, self.live_contour_handle]
            .into_iter()
            .flatten()
            .collect();
        self.live_handle = None;
        self.live_contour_handle = None;
        CmdResult::CommitEntity(entity)
    }

    fn build_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }

        let total_thickness = if let Some(layers) = &self.resolved_layers {
            layers.iter().map(|l| l.thickness).sum()
        } else {
            self.thickness
        };

        let offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        let final_points = if offset.abs() > 1e-9 {
            let directions = engine::get_offset_directions(&points);
            points
                .iter()
                .zip(directions.iter())
                .map(|(&(x, y), &(dx, dy))| (x + dx * offset, y + dy * offset))
                .collect()
        } else {
            points
        };

        let mut pl = LwPolyline::new();
        for (i, (x, y)) in final_points.into_iter().enumerate() {
            let mut v = LwVertex::new(Vector2::new(x, y));
            // Bulge is preserved as-is on the offset axis: an exact offset
            // curve of an arc has a different (but nearby) radius, and this
            // approximation keeps the shape visually correct for the common
            // Center-justified case (offset 0.0, no change needed) while
            // staying serviceable for the off-center case.
            v.bulge = self.bulges.get(i).copied().unwrap_or(0.0);
            pl.add_vertex(v);
        }
        let mut entity = self.plane.place_entity(EntityType::LwPolyline(pl));

        let style_id = self.style_id.clone().unwrap_or_default();
        let layers: Vec<WallLayer> = if let Some(layers) = &self.resolved_layers {
            layers.clone()
        } else {
            vec![WallLayer {
                material: String::new(),
                thickness: self.thickness,
                function: "Structural".to_string(),
                axis_offset: -self.thickness * 0.5,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                layer_id: Uuid::new_v4(),
            }]
        };
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            &style_id,
            self.wall.height,
            self.wall.storey_id,
            &layers,
            &[],
            self.justification, self.wall.phase, self.wall.hatch_override.as_ref());

        entity.common_mut().extended_data.add_record(record);
        Some(entity)
    }

    fn build_contour_entity(&self) -> Option<EntityType> {
        if self.vertices.len() < 2 {
            return None;
        }
        // Follow the same thickness fallback as `build_entity`: once a style
        // is picked, use its resolved layers; before that (or if there are no
        // layers), fall back to a single default-thickness layer so the outline
        // preview always follows the cursor.
        let layer_data: Vec<(f64, f64)> = match self.resolved_layers.as_ref() {
            Some(layers) if !layers.is_empty() => layers
                .iter()
                .map(|l| (l.thickness, l.axis_offset))
                .collect(),
            _ => vec![(self.thickness, -self.thickness * 0.5)],
        };
        let total_thickness = if layer_data.is_empty() {
            0.0
        } else {
            let min_s = layer_data
                .iter()
                .map(|(_, off)| *off)
                .fold(f64::INFINITY, f64::min);
            let max_e = layer_data
                .iter()
                .map(|(th, off)| *off + *th)
                .fold(f64::NEG_INFINITY, f64::max);
            (max_e - min_s).max(0.0)
        };
        let centerline_offset = self.justification.offset(total_thickness);

        let points: Vec<(f64, f64)> = self.vertices
            .iter()
            .map(|pt| {
                let local = self.plane.to_local(*pt);
                (local.x, local.y)
            })
            .collect();

        // Shared WallRepresentation path — outer_contour_2d is produced by the
        // existing contour helpers, so the live draw outline is unchanged.
        // Bulge-aware so curved axes segments keep their arc shape in the
        // preview/final outline (`wall_layer_footprints_with_bulges` consumes
        // the same axis bulges on regeneration).
        let repr = engine::representation::build_wall_representation_with_bulges(
            &points,
            &self.bulges,
            &layer_data,
            centerline_offset,
        );

        let mut pl = LwPolyline::new();
        pl.is_closed = true;
        for (i, (x, y)) in repr.outer_contour_2d.into_iter().enumerate() {
            let mut v = LwVertex::new(Vector2::new(x, y));
            v.bulge = repr.outer_contour_bulges.get(i).copied().unwrap_or(0.0);
            pl.add_vertex(v);
        }

        let entity = self.plane.place_entity(EntityType::LwPolyline(pl));
        // Contour is a visual helper; no XDATA needed (axis carries the truth).
        Some(entity)
    }

    fn sync_live(&self, finish: bool) -> CmdResult {
        let axis = self.build_entity();
        let contour = self.build_contour_entity();

        match (axis, self.live_handle) {
            (Some(a), Some(h_axis)) => {
                if let (Some(c), None) = (&contour, self.live_contour_handle) {
                    // We just gained a contour (e.g. style assigned mid-draw);
                    // replace the single axis with both axis + contour.
                    CmdResult::ReplaceEntity(h_axis, vec![a, c.clone()])
                } else if let (None, Some(h_contour)) = (&contour, self.live_contour_handle) {
                    // Style removed mid-draw? Rare, but handle it by replacing
                    // both with just the axis.
                    CmdResult::ReplaceManyContinue(vec![(h_axis, vec![a]), (h_contour, vec![])])
                } else {
                    let mut updates = vec![(h_axis, a)];
                    if let (Some(c), Some(h_contour)) = (contour, self.live_contour_handle) {
                        updates.push((h_contour, c));
                    }
                    CmdResult::UpdateLiveEntities { updates, finish }
                }
            }
            (Some(a), None) => {
                let mut entities = vec![a];
                if let Some(c) = contour {
                    entities.push(c);
                }
                CmdResult::CommitLiveEntities(entities)
            }
            (None, _) => CmdResult::Cancel,
        }
    }

    /// Like [`Self::sync_live`], but used for edits coming from the live
    /// Properties-panel fields (style/height), which can legitimately fire
    /// before there is anything to preview yet (e.g. only the start point has
    /// been placed). In that case there is no live entity to update/cancel —
    /// just keep the command running and wait for the next point.
    fn sync_live_if_previewable(&self, finish: bool) -> CmdResult {
        if self.vertices.len() < 2 {
            return CmdResult::NeedPoint;
        }
        self.sync_live(finish)
    }

    fn undo_last_vertex(&mut self) -> CmdResult {
        if self.vertices.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.vertices.pop();
        self.bulges.pop();
        CmdResult::NeedPoint
    }

    /// Tangent-continuation bulge (matches `PLINE`'s arc-continue default):
    /// the arc from `prev` to `next` is tangent at `prev` to the direction of
    /// the previous segment (`prev_prev` -> `prev`). Without a previous
    /// segment to derive a tangent from (first placed segment while in arc
    /// mode), there is nothing to be tangent to, so this degrades gracefully
    /// to a straight line (`0.0`) rather than guessing a radius.
    fn compute_tangent_bulge(
        prev_prev: Option<(f64, f64)>,
        prev: (f64, f64),
        next: (f64, f64),
    ) -> f64 {
        let Some(prev_prev) = prev_prev else {
            return 0.0;
        };
        Self::tangent_bulge_from_dir(prev_prev, prev, next)
    }

    fn tangent_bulge_from_dir(
        prev_prev: (f64, f64),
        prev: (f64, f64),
        next: (f64, f64),
    ) -> f64 {
        let dir = (prev.0 - prev_prev.0, prev.1 - prev_prev.1);
        let dir_len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
        let chord = (next.0 - prev.0, next.1 - prev.1);
        let chord_len = (chord.0 * chord.0 + chord.1 * chord.1).sqrt();
        if dir_len < 1e-12 || chord_len < 1e-12 {
            return 0.0;
        }
        // Signed angle from tangent direction to chord (cross/dot atan2).
        let cross = dir.0 * chord.1 - dir.1 * chord.0;
        let dot = dir.0 * chord.0 + dir.1 * chord.1;
        let alpha = cross.atan2(dot);
        // Tangent-chord angle equals half the arc's central angle.
        let theta = 2.0 * alpha;
        (theta / 4.0).tan()
    }

    /// Bulge of chord `start`→`end` for the circular arc that also passes
    /// through `through`. Collinear points yield `0.0`.
    fn bulge_through_three(start: (f64, f64), through: (f64, f64), end: (f64, f64)) -> f64 {
        let (ax, ay) = start;
        let (bx, by) = through;
        let (cx, cy) = end;
        let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
        if d.abs() < 1e-18 {
            return 0.0;
        }
        let a2 = ax * ax + ay * ay;
        let b2 = bx * bx + by * by;
        let c2 = cx * cx + cy * cy;
        let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
        let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
        let ra = (ax - ux, ay - uy);
        let rb = (bx - ux, by - uy);
        let rc = (cx - ux, cy - uy);
        let cross_ac = ra.0 * rc.1 - ra.1 * rc.0;
        let dot_ac = ra.0 * rc.0 + ra.1 * rc.1;
        let mut theta = cross_ac.atan2(dot_ac);
        let cross_ab = ra.0 * rb.1 - ra.1 * rb.0;
        let dot_ab = ra.0 * rb.0 + ra.1 * rb.1;
        let theta_ab = cross_ab.atan2(dot_ab);
        let on_arc = if theta >= 0.0 {
            theta_ab >= -1e-9 && theta_ab <= theta + 1e-9
        } else {
            theta_ab <= 1e-9 && theta_ab >= theta - 1e-9
        };
        if !on_arc {
            if theta >= 0.0 {
                theta -= 2.0 * std::f64::consts::PI;
            } else {
                theta += 2.0 * std::f64::consts::PI;
            }
        }
        (theta / 4.0).tan()
    }

    fn pending_arc_bulge(&self, start: DVec3, end: DVec3) -> f64 {
        let start_l = self.plane.to_local(start);
        let end_l = self.plane.to_local(end);
        let a = DVec2::new(start_l.x, start_l.y);
        let b = DVec2::new(end_l.x, end_l.y);
        // Default tangent is *not* axis-aligned so a typical first horizontal
        // segment still gets a visible arc (PLINE's +X default would be 0).
        let tangent = self
            .last_tangent
            .unwrap_or_else(|| DVec2::new(1.0, 1.0).normalize_or_zero());
        crate::modules::draw::draw::polyline::compute_bulge(a, tangent, b)
    }
}

impl CadCommand for WallCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "AEC_WALL"
    }

    fn prompt(&self) -> String {
        let mode = if self.arc_mode {
            crate::tr!("aec", "wall-mode-arc")
        } else {
            String::new()
        };
        if self.vertices.is_empty() {
            crate::tr!(
                "aec",
                "wall-prompt-start",
                justification = self.justification.as_str()
            )
        } else if self.no_style_warning {
            crate::tr!("aec", "wall-prompt-need-style")
        } else {
            crate::tr!(
                "aec",
                "wall-prompt-next",
                mode = mode.as_str(),
                justification = self.justification.as_str(),
                count = self.vertices.len()
            )
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.vertices.is_empty() {
            return Vec::new();
        }
        let arc_toggle = if self.arc_mode {
            CmdOption::new("Line", "L")
        } else {
            CmdOption::new("Arc", "A")
        };
        vec![
            CmdOption::new("Undo", "U"),
            arc_toggle,
            CmdOption::enter("Done"),
        ]
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        if ctrl && !self.ctrl_was_down {
            self.justification = self.justification.next();
            // In a real CLI we would use CmdResult::Log, but set_ctrl doesn't
            // return it. Toggling the prompt is the next best thing for
            // live feedback.
        }
        self.ctrl_was_down = ctrl;
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        if self.vertices.is_empty() {
            return vec![];
        }

        // Axis rubber band: pending segment from the last placed point to
        // the cursor (the committed vertices already render as the live
        // axis polyline, same convention as `PlineCommand::on_mouse_move`).
        // In arc mode, tessellate the tangent-continuation arc instead of a
        // straight line so the curve is visible before the point is placed.
        let last_world = *self.vertices.last().unwrap();
        let axis_wire = if self.arc_mode {
            let last_local = self.plane.to_local(last_world);
            let cursor_local = self.plane.to_local(pt);
            let bulge = self.pending_arc_bulge(last_world, pt);
            let arc_pts = tessellate_bulge_segment(
                (last_local.x, last_local.y),
                (cursor_local.x, cursor_local.y),
                bulge,
                24,
            );
            let world_pts: Vec<[f32; 3]> = arc_pts
                .iter()
                .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array())
                .collect();
            WireModel::solid("rubber_band_axis".into(), world_pts, WireModel::CYAN, false)
        } else {
            WireModel::solid(
                "rubber_band_axis".into(),
                vec![
                    last_world.as_vec3().to_array(),
                    pt.as_vec3().to_array(),
                ],
                WireModel::CYAN,
                false,
            )
        };
        let mut wires = vec![axis_wire];

        // Outline rubber band: the wall's outer contour via the shared
        // WallRepresentation builder, computed on the committed vertices plus
        // the not-yet-placed cursor point. Same thickness/justification fallback
        // as `build_contour_entity` so the outline tracks the cursor from the
        // first point onward, including before a style is chosen.
        let mut temp_vertices = self.vertices.clone();
        temp_vertices.push(pt);
        let mut temp_bulges = self.bulges.clone();
        temp_bulges.resize(self.vertices.len(), 0.0);
        if self.arc_mode && self.vertices.len() >= 1 {
            let bulge = self.pending_arc_bulge(*self.vertices.last().unwrap(), pt);
            if let Some(last) = temp_bulges.last_mut() {
                *last = bulge;
            }
        }
        temp_bulges.push(0.0);
        if temp_vertices.len() >= 2 {
            let layer_data: Vec<(f64, f64)> = match self.resolved_layers.as_ref() {
                Some(layers) if !layers.is_empty() => layers
                    .iter()
                    .map(|l| (l.thickness, l.axis_offset))
                    .collect(),
                _ => vec![(self.thickness, -self.thickness * 0.5)],
            };
            let total_thickness = if layer_data.is_empty() {
            0.0
        } else {
            let min_s = layer_data
                .iter()
                .map(|(_, off)| *off)
                .fold(f64::INFINITY, f64::min);
            let max_e = layer_data
                .iter()
                .map(|(th, off)| *off + *th)
                .fold(f64::NEG_INFINITY, f64::max);
            (max_e - min_s).max(0.0)
        };
            let centerline_offset = self.justification.offset(total_thickness);
            let points: Vec<(f64, f64)> = temp_vertices
                .iter()
                .map(|p| {
                    let local = self.plane.to_local(*p);
                    (local.x, local.y)
                })
                .collect();
            // Full outer contour (not drag_ghost) so justification stays correct
            // and the rubber-band matches the committed contour entity.
            let repr = engine::representation::build_wall_representation_with_bulges(
                &points,
                &temp_bulges,
                &layer_data,
                centerline_offset,
            );
            if !repr.outer_contour_2d.is_empty() {
                let mut world_pts: Vec<[f32; 3]> = repr
                    .outer_contour_2d
                    .iter()
                    .map(|&(x, y)| self.plane.to_world(DVec3::new(x, y, 0.0)).as_vec3().to_array())
                    .collect();
                if let Some(first) = world_pts.first().copied() {
                    world_pts.push(first);
                }
                wires.push(WireModel::solid(
                    "rubber_band_contour".into(),
                    world_pts,
                    WireModel::CYAN,
                    false,
                ));
            }
        }

        wires
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(&last) = self.vertices.last() {
            if last.distance(pt) < 1e-9 {
                return CmdResult::NeedPoint;
            }
            let bulge = if self.arc_mode {
                self.pending_arc_bulge(last, pt)
            } else {
                0.0
            };
            if let Some(slot) = self.bulges.last_mut() {
                *slot = bulge;
            }
        }
        self.vertices.push(pt);
        self.bulges.push(0.0);
        if self.vertices.len() >= 2 {
            self.commit_current_segment()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_entities_committed(&mut self, scene: &mut Scene, handles: &[Handle]) {
        let Some(&wall_handle) = handles.first() else {
            return;
        };
        let is_wall = scene
            .document
            .get_entity(wall_handle)
            .is_some_and(|e| wall_from_entity(e).is_some());
        if !is_wall {
            return;
        }
        if !self.pending_preview_companions.is_empty() {
            erase_wall_live_preview_companions(
                scene,
                wall_handle,
                &self.pending_preview_companions,
            );
            self.pending_preview_companions.clear();
        }
        auto_join_committed_wall_segment(
            scene,
            wall_handle,
            self.last_committed,
            self.library.as_ref(),
        );
        let _ = regenerate_wall_representation(scene, wall_handle, self.library.as_ref());
        self.last_committed = Some(wall_handle);
    }

    fn set_live_handles(&mut self, handles: Vec<Handle>) {
        self.live_handle = handles.first().copied();
        self.live_contour_handle = handles.get(1).copied();
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[Handle]) {
        if Some(old) == self.live_handle {
            self.live_handle = new_handles.first().copied();
            if new_handles.len() > 1 {
                self.live_contour_handle = Some(new_handles[1]);
            }
        } else if Some(old) == self.live_contour_handle {
            if new_handles.is_empty() {
                self.live_contour_handle = None;
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.start_dimension_prompt()
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.vertices.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn on_space_change(&mut self) -> CmdResult {
        self.on_enter()
    }

    fn wants_text_input(&self) -> bool {
        !self.vertices.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.vertices.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "U" | "UNDO" => Some(self.undo_last_vertex()),
            "A" | "ARC" => {
                self.arc_mode = true;
                Some(CmdResult::NeedPoint)
            }
            "L" | "LINE" => {
                self.arc_mode = false;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if !self.vertices.is_empty() {
            Some(self.undo_last_vertex())
        } else {
            None
        }
    }

    fn live_properties(&self) -> Option<crate::command::LiveCommandProperties> {
        use crate::command::{LiveCommandField, LiveCommandProperties, LiveFieldValue};

        let style_name = match &self.style_id {
            Some(id) => self
                .library
                .as_ref()
                .and_then(|lib| lib.wall_styles.iter().find(|ws| &ws.style.id == id))
                .map(|ws| ws.style.name.clone())
                .unwrap_or_else(|| id.clone()),
            None => String::new(),
        };

        Some(LiveCommandProperties {
            title: crate::t!("Wall").into_owned(),
            fields: vec![
                LiveCommandField {
                    label: crate::t!("Style").into_owned(),
                    field_id: "wall_style",
                    value: LiveFieldValue::Picker(style_name),
                },
                LiveCommandField {
                    label: crate::t!("Height").into_owned(),
                    field_id: "wall_height",
                    value: LiveFieldValue::Number(self.wall.height),
                },
                LiveCommandField {
                    label: crate::t!("Justification").into_owned(),
                    field_id: "wall_justification",
                    value: LiveFieldValue::Choice {
                        selected: self.justification.as_str().to_string(),
                        options: vec![
                            "Interior".to_string(),
                            "Center".to_string(),
                            "Exterior".to_string(),
                        ],
                    },
                },
            ],
        })
    }

    fn live_property_id(&self, field_id: &str) -> Option<String> {
        match field_id {
            "wall_style" => self.style_id.clone(),
            _ => None,
        }
    }

    fn apply_live_property(
        &mut self,
        field_id: &str,
        value: crate::command::LiveFieldValue,
    ) -> CmdResult {
        use crate::command::LiveFieldValue;

        match (field_id, value) {
            ("wall_style", LiveFieldValue::Picker(style_id)) => {
                let Some(lib) = &self.library else {
                    return CmdResult::NeedPoint;
                };
                let Some(style) = lib
                    .wall_styles
                    .iter()
                    .find(|s| s.style.id == style_id)
                else {
                    return CmdResult::NeedPoint;
                };

                self.style_id = Some(style.style.id.clone());
                self.no_style_warning = false;
                self.resolved_layers =
                    resolve_wall_style_layers(lib, &style.style.id, None);

                if self.vertices.len() >= 2 {
                    self.commit_current_segment()
                } else {
                    CmdResult::NeedPoint
                }
            }
            ("wall_height", LiveFieldValue::Number(h)) => {
                self.wall.height = h;
                self.height_live_set = true;
                self.sync_live_if_previewable(false)
            }
            ("wall_justification", LiveFieldValue::Choice { selected, .. })
            | ("wall_justification", LiveFieldValue::Text(selected)) => {
                self.justification = WallJustification::from_str(&selected);
                self.sync_live_if_previewable(false)
            }
            _ => CmdResult::NeedPoint,
        }
    }
}

/// Converts a [`LayerFunction`] to the plain string used in XDATA/dispatch.
fn layer_function_to_str(f: &LayerFunction) -> String {
    match f {
        LayerFunction::Structural => "Structural".to_string(),
        LayerFunction::Insulation => "Insulation".to_string(),
        LayerFunction::Finish => "Finish".to_string(),
        LayerFunction::Other(s) => s.clone(),
    }
}

/// Resolve a wall style's effective layers into concrete [`WallLayer`]s.
///
/// Formula thicknesses are evaluated with `"BB"` = `bb` when provided, otherwise
/// the sum of fixed layer thicknesses (+ gaps) from the style
/// ([`base_width_from_layers`]). Invalid formulas fall back to `0.0` thickness
/// (see [`ResolvedLayer::formula_error`]).
///
/// When `use_material_names` is true, material fields store the library display
/// name; otherwise the raw material id is kept (properties/style-picker path).
pub fn resolve_wall_style_layers(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
) -> Option<Vec<WallLayer>> {
    resolve_wall_style_layers_ex(lib, style_id, bb, true)
}

/// Same as [`resolve_wall_style_layers`] but keeps material ids instead of names.
pub fn resolve_wall_style_layers_ids(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
) -> Option<Vec<WallLayer>> {
    resolve_wall_style_layers_ex(lib, style_id, bb, false)
}

fn resolve_wall_style_layers_ex(
    lib: &StyleLibrary,
    style_id: &str,
    bb: Option<f64>,
    use_material_names: bool,
) -> Option<Vec<WallLayer>> {
    let style_map: HashMap<String, WallStyle> = lib
        .wall_styles
        .iter()
        .map(|s| (s.style.id.clone(), s.clone()))
        .collect();

    // Need unresolved layers first when BB is not supplied.
    let unresolved = super::engine::wall_style::effective_layers(&style_map, &style_id.to_string())
        .ok()?;
    let bb = bb.unwrap_or_else(|| base_width_from_layers(&unresolved));
    let resolved = effective_layers_for_wall_bb(&style_map, &style_id.to_string(), bb).ok()?;

    Some(
        resolved
            .into_iter()
            .map(|layer| {
                if use_material_names {
                    resolved_layer_to_wall_layer(lib, layer)
                } else {
                    resolved_layer_to_wall_layer_raw(layer)
                }
            })
            .collect(),
    )
}

/// Map a [`ResolvedLayer`] to a runtime [`WallLayer`], preferring the library
/// material display name when available.
fn resolved_layer_to_wall_layer(lib: &StyleLibrary, layer: ResolvedLayer) -> WallLayer {
    let mat_name = lib
        .materials
        .iter()
        .find(|m| m.id == layer.material_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| layer.material_id.clone());
    WallLayer {
        material: mat_name,
        thickness: layer.thickness,
        function: layer_function_to_str(&layer.function),
        axis_offset: layer.axis_offset,
        bottom_offset: layer.bottom_offset,
        top_offset: layer.top_offset,
        layer_override: layer.layer_override,
        hatch_override: layer.hatch_override,
        layer_id: layer.layer_id,
    }
}

/// Like [`resolved_layer_to_wall_layer`] but keeps `material_id` as the material
/// string (used by the properties/style-picker paths that store ids).
fn resolved_layer_to_wall_layer_raw(layer: ResolvedLayer) -> WallLayer {
    WallLayer {
        material: layer.material_id,
        thickness: layer.thickness,
        function: layer_function_to_str(&layer.function),
        axis_offset: layer.axis_offset,
        bottom_offset: layer.bottom_offset,
        top_offset: layer.top_offset,
        layer_override: layer.layer_override,
        hatch_override: layer.hatch_override,
        layer_id: layer.layer_id,
    }
}

/// Parses a plain string (as entered on the command line) into a
/// [`LayerFunction`], defaulting to `Structural` for empty input and falling
/// back to `Other(..)` for anything unrecognized.
fn parse_layer_function(s: &str) -> LayerFunction {
    match s.trim() {
        "" | "Structural" | "structural" => LayerFunction::Structural,
        "Insulation" | "insulation" => LayerFunction::Insulation,
        "Finish" | "finish" => LayerFunction::Finish,
        other => LayerFunction::Other(other.to_string()),
    }
}

/// Turns a human-entered name into a stable, filesystem/XDATA-safe id
/// (lowercase, non-alphanumeric runs collapsed to `_`).
pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = true; // suppress a leading separator
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    if slug.is_empty() {
        "item".to_string()
    } else {
        slug
    }
}

/// Builds a new, globally unique library id (`<prefix>_<slug>_<suffix>`) for
/// a freshly created material/wall style.
///
/// Ids used to be purely name-derived (`<prefix>_<slugified name>`), which
/// meant two libraries created independently (e.g. in different projects)
/// could end up with the *same* id for wall styles/materials that merely
/// share a name but have different layer definitions. If those libraries
/// are later mixed (project copied, wall pasted across drawings, ...), the
/// wrong entry silently wins on lookup. Appending a short random suffix
/// keeps ids human-readable while making cross-project collisions
/// practically impossible; existing ids (loaded from disk, or passed in
/// when editing) are left untouched so old data keeps resolving correctly.
pub(crate) fn unique_id(prefix: &str, name: &str) -> String {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}_{}_{}", slugify(name), &suffix[..8])
}

/// Step of an in-progress `AEC_MATERIAL` command.
enum MaterialStep {
    Name,
    Hatch { name: String },
    Color { name: String, hatch: String },
    LineType { name: String, hatch: String, color: u32 },
}

/// `AEC_MATERIAL` — create (or update) a material in the AEC style library,
/// prompting step by step for name, hatch pattern, line color and line type.
pub struct MaterialCommand {
    step: MaterialStep,
}

impl MaterialCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: MaterialStep::Name,
        }
    }
}

impl CadCommand for MaterialCommand {
    fn name(&self) -> &'static str {
        "AEC_MATERIAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            MaterialStep::Name => crate::tr!("aec", "material-prompt-name"),
            MaterialStep::Hatch { .. } => crate::tr!("aec", "material-prompt-hatch"),
            MaterialStep::Color { .. } => crate::tr!("aec", "material-prompt-color"),
            MaterialStep::LineType { .. } => crate::tr!("aec", "material-prompt-linetype"),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            MaterialStep::Name => {
                if t.is_empty() {
                    // A material needs a name; keep prompting.
                    return Some(CmdResult::NeedPoint);
                }
                self.step = MaterialStep::Hatch {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Hatch { name } => {
                let hatch = if t.is_empty() {
                    "ANSI31".to_string()
                } else {
                    t.to_string()
                };
                self.step = MaterialStep::Color {
                    name: name.clone(),
                    hatch,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::Color { name, hatch } => {
                let color = u32::from_str_radix(t.trim_start_matches('#'), 16).unwrap_or(0);
                self.step = MaterialStep::LineType {
                    name: name.clone(),
                    hatch: hatch.clone(),
                    color,
                };
                Some(CmdResult::NeedPoint)
            }
            MaterialStep::LineType { name, hatch, color } => {
                let line_type = if t.is_empty() {
                    "Continuous".to_string()
                } else {
                    t.to_string()
                };
                Some(CmdResult::Dispatch(format!(
                    "AEC_MATERIAL_ADD {name}|{hatch}|{color:06X}|{line_type}"
                )))
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_MATERIAL_ADD name|hatch|color_hex|line_type` — the non-interactive
/// handler `MaterialCommand` dispatches to once all fields are collected;
/// upserts the material (by name → stable id) into the style library and
/// persists it.
pub fn aec_material_add(command_line: &mut CommandLine, args: &str) {
    let parts: Vec<&str> = args.split('|').collect();
    let [name, hatch, color_hex, line_type] = parts.as_slice() else {
        command_line.push_error(&crate::tr!("aec", "material-malformed"));
        return;
    };
    let color = u32::from_str_radix(color_hex, 16).unwrap_or(0);

    let mut lib = load_or_seed();
    // Re-use the id of an existing material with the same name so re-running
    // AEC_MATERIAL_ADD on the same name still updates it in place; only a
    // genuinely new name gets a fresh, globally unique id (see `unique_id`).
    let id = lib
        .materials
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .map(|m| m.id.clone())
        .unwrap_or_else(|| unique_id("mat", name));
    lib.upsert_material(Material::new(
        id,
        name.to_string(),
        hatch.to_string(),
        color,
        line_type.to_string(),
    ));
    match engine::library::save_to_default_path(&lib) {
        Ok(()) => command_line.push_info(&crate::tr!(
            "aec",
            "material-saved",
            name = name.to_string(),
            hatch = hatch.to_string(),
            color = format!("{color:06X}"),
            line_type = line_type.to_string()
        )),
        Err(e) => command_line.push_error(&crate::tr!(
            "aec",
            "material-save-failed",
            error = e.to_string()
        )),
    }
}

/// Step of an in-progress `AEC_STYLE` command.
enum StyleStep {
    Name,
    Parent {
        name: String,
    },
    /// Collecting layers; `layers` accumulates `(material_name, thickness, function)`.
    LayerMaterial {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
    },
    LayerThickness {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
    },
    LayerFunctionStep {
        name: String,
        parent: Option<String>,
        layers: Vec<(String, f64, LayerFunction)>,
        material: String,
        thickness: f64,
    },
}

/// `AEC_STYLE` — create (or update) a wall style in the AEC style library:
/// name, optional parent style (for single-parent inheritance), then a loop
/// collecting material/thickness/function per layer (blank material name
/// ends the loop).
pub struct StyleCommand {
    step: StyleStep,
}

impl StyleCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            step: StyleStep::Name,
        }
    }

    fn finish(name: &str, parent: &Option<String>, layers: &[(String, f64, LayerFunction)]) -> CmdResult {
        let parent_part = parent.clone().unwrap_or_default();
        let layers_part = layers
            .iter()
            .map(|(mat, thick, func)| format!("{mat}:{thick}:{}", layer_function_to_str(func)))
            .collect::<Vec<_>>()
            .join(";");
        CmdResult::Dispatch(format!("AEC_STYLE_ADD {name}|{parent_part}|{layers_part}"))
    }
}

impl CadCommand for StyleCommand {
    fn name(&self) -> &'static str {
        "AEC_STYLE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            StyleStep::Name => crate::tr!("aec", "style-prompt-name"),
            StyleStep::Parent { .. } => crate::tr!("aec", "style-prompt-parent"),
            StyleStep::LayerMaterial { layers, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-material",
                n = { (layers.len() + 1) as i32 }
            ),
            StyleStep::LayerThickness { material, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-thickness",
                material = material.as_str()
            ),
            StyleStep::LayerFunctionStep { material, .. } => crate::tr!(
                "aec",
                "style-prompt-layer-function",
                material = material.as_str()
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            StyleStep::Name => {
                if t.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.step = StyleStep::Parent {
                    name: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::Parent { name } => {
                let parent = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent,
                    layers: Vec::new(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerMaterial { name, parent, layers } => {
                if t.is_empty() {
                    // No (more) layers: finish, possibly inheriting layers from
                    // the parent style if none were entered here.
                    return Some(Self::finish(name, parent, layers));
                }
                self.step = StyleStep::LayerThickness {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: t.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerThickness {
                name,
                parent,
                layers,
                material,
            } => {
                let thickness = WallCommand::parse_dimension(t, 0.2);
                self.step = StyleStep::LayerFunctionStep {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers: layers.clone(),
                    material: material.clone(),
                    thickness,
                };
                Some(CmdResult::NeedPoint)
            }
            StyleStep::LayerFunctionStep {
                name,
                parent,
                layers,
                material,
                thickness,
            } => {
                let function = parse_layer_function(t);
                let mut layers = layers.clone();
                layers.push((material.clone(), *thickness, function));
                self.step = StyleStep::LayerMaterial {
                    name: name.clone(),
                    parent: parent.clone(),
                    layers,
                };
                Some(CmdResult::NeedPoint)
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_STYLE_ADD name|parent|mat1:thick1:func1;mat2:thick2:func2...` — the
/// non-interactive handler `StyleCommand` dispatches to once all fields are
/// collected; upserts the wall style (by name → stable id) into the style
/// library and persists it. An empty layer list inherits layers from the
/// parent style at resolution time (see `effective_layers`).
pub fn aec_style_add(command_line: &mut CommandLine, args: &str) {
    let mut parts = args.splitn(3, '|');
    let (Some(name), Some(parent_raw), Some(layers_raw)) =
        (parts.next(), parts.next(), parts.next())
    else {
        command_line.push_error(&crate::tr!("aec", "style-malformed"));
        return;
    };

    let mut lib = load_or_seed();

    let parent_style_id = if parent_raw.is_empty() {
        None
    } else {
        match lib
            .wall_styles
            .iter()
            .find(|s| s.style.name.eq_ignore_ascii_case(parent_raw))
        {
            Some(p) => Some(p.style.id.clone()),
            None => {
                command_line.push_error(&crate::tr!(
                    "aec",
                    "style-unknown-parent",
                    parent = parent_raw
                ));
                None
            }
        }
    };

    let mut layers = Vec::new();
    if !layers_raw.is_empty() {
        for entry in layers_raw.split(';') {
            // `material:thickness:function[:role_tag]`. The role tag is an
            // optional, purely informational 4th field (e.g. "Tragschale").
            let fields: Vec<&str> = entry.splitn(4, ':').collect();
            if fields.len() < 3 {
                continue;
            }
            let mat_name = fields[0];
            let thick_str = fields[1];
            let func_str = fields[2];
            let role_tag = fields.get(3).filter(|s| !s.is_empty()).map(|s| s.to_string());
            let material_id = lib
                .materials
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(mat_name))
                .map(|m| m.id.clone())
                .unwrap_or_else(|| format!("mat_{}", slugify(mat_name)));
            let thickness = LayerValue::parse_str(thick_str);
            layers.push(Layer {
                material_id,
                thickness,
                function: parse_layer_function(func_str),
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag,
                layer_id: Uuid::new_v4(),
            });
        }
    }

    // Re-use the id of an existing wall style with the same name so
    // re-running AEC_STYLE_ADD on the same name still updates it in place;
    // only a genuinely new name gets a fresh, globally unique id.
    let id = lib
        .wall_styles
        .iter()
        .find(|s| s.style.name.eq_ignore_ascii_case(name))
        .map(|s| s.style.id.clone())
        .unwrap_or_else(|| unique_id("style", name));
    lib.upsert_wall_style(WallStyle {
        style: Style {
            id,
            name: name.to_string(),
            object_kind: "Wall".to_string(),
            parent_style_id,
        },
        layers,
    display_profiles: std::collections::HashMap::new(),
    });

    match engine::library::save_to_default_path(&lib) {
        Ok(()) => {
            let count = lib.wall_styles
                .iter()
                .find(|s| s.style.name == name)
                .map(|s| s.layers.len())
                .unwrap_or(0);
            command_line.push_info(&crate::tr!(
                "aec",
                "style-saved",
                name = name,
                count = count
            ));
        }
        Err(e) => command_line.push_error(&crate::tr!(
            "aec",
            "style-save-failed",
            error = e.to_string()
        )),
    }
}

/// `AEC_ROOM` — detect a closed wall loop (or demo rectangle) + ROOM XDATA.
pub fn aec_room(scene: &mut Scene, command_line: &mut CommandLine) {
    let wall_segments = collect_wall_segments(&scene.document);
    let (pts, detected) = match find_closed_loop(&wall_segments, 1e-3) {
        Some(loop_pts) => (loop_pts, true),
        None => (
            vec![(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)],
            false,
        ),
    };

    let room = Room::from_polygon("Office 101", &pts, 2.8, 0);

    let mut pl = LwPolyline::new();
    pl.is_closed = true;
    for (x, y) in &pts {
        pl.add_vertex(LwVertex::new(Vector2::new(*x, *y)));
    }

    let handle = scene.add_entity(EntityType::LwPolyline(pl));

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("ROOM".to_string()));
    record.add_value(XDataValue::String(room.name.clone()));
    record.add_value(XDataValue::Real(room.area));
    record.add_value(XDataValue::Real(room.perimeter));
    record.add_value(XDataValue::Real(room.volume));
    record.add_value(XDataValue::Integer32(room.storey_id as i32));

    write_aec_record(&mut scene.document, handle, record);
    scene.bump_geometry();
    if detected {
        command_line.push_info(&crate::tr!(
            "aec",
            "room-detected",
            name = room.name.as_str(),
            count = pts.len(),
            handle = handle.to_string()
        ));
    } else {
        command_line.push_info(&crate::tr!(
            "aec",
            "room-demo",
            name = room.name.as_str(),
            handle = handle.to_string()
        ));
    }
}

/// Build a `STOREY` XDATA record (id + name/elevation/height).
fn storey_record(storey_id: u32, storey: &Storey) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("STOREY".to_string()));
    record.add_value(XDataValue::Integer32(storey_id as i32));
    record.add_value(XDataValue::String(storey.name.clone()));
    record.add_value(XDataValue::Real(storey.elevation));
    record.add_value(XDataValue::Real(storey.height));
    record
}

/// Parse a `STOREY` XDATA record into `(storey_id, Storey)`.
pub fn storey_from_entity(entity: &EntityType) -> Option<(u32, Storey)> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 5 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "STOREY" {
        return None;
    }
    let storey_id = match v[1] {
        XDataValue::Integer32(i) => i as u32,
        _ => return None,
    };
    let name = match &v[2] {
        XDataValue::String(s) => s.clone(),
        _ => return None,
    };
    let elevation = match v[3] {
        XDataValue::Real(r) => r,
        _ => return None,
    };
    let height = match v[4] {
        XDataValue::Real(r) => r,
        _ => return None,
    };
    Some((storey_id, Storey::new(name, elevation, height)))
}

/// Document handle of the entity carrying `STOREY` XDATA for `storey_id`.
pub fn find_storey_handle(doc: &CadDocument, storey_id: u32) -> Option<Handle> {
    for entity in doc.entities() {
        if let Some((id, _)) = storey_from_entity(entity) {
            if id == storey_id {
                return Some(entity.common().handle);
            }
        }
    }
    None
}

/// Ensure a storey entity exists for `storey_id`. Creates a POINT carrier with
/// `STOREY` XDATA when missing (using `storey` metadata, or a default Level N).
pub fn ensure_storey_entity(scene: &mut Scene, storey_id: u32, storey: Option<&Storey>) -> Handle {
    if let Some(h) = find_storey_handle(&scene.document, storey_id) {
        return h;
    }
    let s = storey.cloned().unwrap_or_else(|| {
        Storey::new(
            format!("Level {}", storey_id + 1),
            (storey_id as f64) * 3.0,
            3.0,
        )
    });
    let point = EntityType::Point(Point::at(Vector3::new(0.0, 0.0, s.elevation)));
    let handle = scene.add_entity(point);
    write_aec_record(&mut scene.document, handle, storey_record(storey_id, &s));
    handle
}

/// Wall axis handles currently indexed as children of the storey entity.
/// Filters `CHILD_HANDLES` down to entities that parse as `WALL`.
pub fn walls_for_storey(scene: &Scene, storey_handle: Handle) -> Vec<Handle> {
    let mut out = Vec::new();
    for child in engine::owner_index::children_of(&scene.document, storey_handle) {
        let Some(entity) = scene.document.get_entity(child) else {
            continue;
        };
        if wall_from_entity(entity).is_some() {
            out.push(child);
        }
    }
    out
}

/// Register `wall_handle` under its current `storey_id` owner index.
pub fn register_wall_in_storey(scene: &mut Scene, wall_handle: Handle) {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return;
    };
    let Some(wall) = wall_from_entity(entity) else {
        return;
    };
    let storey_h = ensure_storey_entity(scene, wall.storey_id, None);
    engine::owner_index::add_child(&mut scene.document, storey_h, wall_handle);
}

/// Drop `wall_handle` from its current storey owner index (if any).
pub fn unregister_wall_from_storey(scene: &mut Scene, wall_handle: Handle) {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return;
    };
    let Some(wall) = wall_from_entity(entity) else {
        return;
    };
    if let Some(storey_h) = find_storey_handle(&scene.document, wall.storey_id) {
        engine::owner_index::remove_child(&mut scene.document, storey_h, wall_handle);
    }
}

/// Change a wall's `storey_id`, reparenting the owner-index entry between
/// storey entities and rewriting WALL XDATA.
pub fn set_wall_storey(scene: &mut Scene, wall_handle: Handle, new_storey_id: u32) -> bool {
    let Some(entity) = scene.document.get_entity(wall_handle) else {
        return false;
    };
    let Some(mut wall) = wall_from_entity(entity) else {
        return false;
    };
    if wall.storey_id == new_storey_id {
        // Still ensure membership is recorded.
        let storey_h = ensure_storey_entity(scene, new_storey_id, None);
        engine::owner_index::add_child(&mut scene.document, storey_h, wall_handle);
        return true;
    }
    if let Some(old_h) = find_storey_handle(&scene.document, wall.storey_id) {
        engine::owner_index::remove_child(&mut scene.document, old_h, wall_handle);
    }
    wall.storey_id = new_storey_id;
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    for v in wall_record(
        &wall.style_id,
        wall.height,
        wall.storey_id,
        &wall.layers,
        &wall.derived_handles,
        wall.justification, wall.phase, wall.hatch_override.as_ref()) {
        record.add_value(v);
    }
    if !write_aec_record(&mut scene.document, wall_handle, record) {
        return false;
    }
    let new_h = ensure_storey_entity(scene, new_storey_id, None);
    engine::owner_index::add_child(&mut scene.document, new_h, wall_handle);
    true
}

/// Before erasing entities, drop wall axes from their storey owner indexes
/// and clear symmetric join peer links.
pub fn unregister_walls_from_storeys(scene: &mut Scene, handles: &[Handle]) {
    for handle in handles {
        unregister_wall_from_storey(scene, *handle);
        unlink_all_wall_peers(scene, *handle);
    }
}

/// Remove `wall` from every peer's `JOINED_PEERS` list and clear its own.
pub fn unlink_all_wall_peers(scene: &mut Scene, wall: Handle) {
    let peers = engine::owner_index::peers_of(&scene.document, wall);
    for peer in peers {
        engine::owner_index::unlink_peers(&mut scene.document, wall, peer);
    }
}

/// `AEC_STOREY` — append a storey (document entity + in-memory scaffold list).
pub fn aec_storey(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut storeys = STOREYS.lock().unwrap();
    let next_id = storeys.len() as u32;
    let new_storey = Storey::new(
        format!("Level {}", next_id + 1),
        (next_id as f64) * 3.0,
        3.0,
    );
    storeys.push(new_storey.clone());
    drop(storeys);

    let handle = ensure_storey_entity(scene, next_id, Some(&new_storey));
    scene.bump_geometry();

    command_line.push_info(&crate::tr!(
        "aec",
        "storey-added",
        name = new_storey.name.as_str(),
        elevation = format!("{}", new_storey.elevation),
        handle = handle.to_string()
    ));
}

/// Expand an erase/delete selection with each wall's `derived_handles`
/// (contour/hatch/solid entities generated by [`regenerate_wall_representation`])
/// so deleting a `WALL` entity also removes its rendered representation.
///
/// Deduplicates and skips any handle not present in the document (already erased).
pub fn expand_with_wall_derived_handles(scene: &Scene, handles: &mut Vec<Handle>) {
    let mut extra = Vec::new();
    for handle in handles.iter() {
        let owner = resolve_wall_package(scene, *handle);
        extra.push(owner);
        let Some(entity) = scene.document.get_entity(owner) else {
            continue;
        };
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        let is_wall = matches!(
            record.values.first(),
            Some(XDataValue::String(kind)) if kind == "WALL"
        );
        if !is_wall {
            continue;
        }
        if let Some(v2) = wall_from_entity(entity) {
            extra.extend(v2.derived_handles.iter().copied());
        }
    }
    for h in extra {
        if !handles.contains(&h) && scene.document.get_entity(h).is_some() {
            handles.push(h);
        }
    }
}

/// `AEC_WALL_REFRESH` — migration path for walls created before the
/// contour/hatch/solid representation existed: rebuild it for every
/// `WALL` entity in the document that doesn't already carry a
/// `derived_handles` list (new walls skip a redundant rebuild).
pub fn aec_wall_refresh(scene: &mut Scene, command_line: &mut CommandLine, library_override: Option<&StyleLibrary>) {
    let candidates: Vec<Handle> = scene
        .document
        .entities()
        .filter_map(|entity| {
            let record = read_aec_record(entity)?;
            match record.values.first() {
                Some(XDataValue::String(kind)) if kind == "WALL" => {
                    // Skip axes that already have derived entities so a
                    // refresh doesn't double-build representations for walls
                    // that still hold a valid package.
                    let wall = wall_from_entity(entity)?;
                    if wall.derived_handles.is_empty() {
                        Some(entity.common().handle)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect();

    let mut refreshed = 0usize;
    for handle in candidates {
        if regenerate_wall_representation(scene, handle, library_override).is_ok() {
            refreshed += 1;
        }
    }
    scene.bump_geometry();
    command_line.push_info(&crate::tr!(
        "aec",
        "wall-refresh",
        count = refreshed
    ));
}

/// `AEC_ROOMSCHEDULE` — scan ROOM XDATA and build a real TABLE entity.
pub fn aec_room_schedule(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut rooms = Vec::new();
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        if let Some(XDataValue::String(kind)) = record.values.first() {
            if kind == "ROOM" && record.values.len() >= 6 {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                let name = if let XDataValue::String(s) = &record.values[1] {
                    s.clone()
                } else {
                    "Unknown".to_string()
                };
                let area = if let XDataValue::Real(r) = record.values[2] {
                    r
                } else {
                    0.0
                };
                let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                    i
                } else {
                    0
                };
                rooms.push((name, area, storey_id));
            }
        }
    }

    if rooms.is_empty() {
        command_line.push_info(&crate::tr!("aec", "no-rooms"));
        return;
    }

    // Real TABLE entity: header row + one row per room.
    let row_count = rooms.len() + 1;
    let mut table = Table::new(Vector3::ZERO, row_count, 3);
    let hdr_name = crate::tr!("aec", "schedule-name");
    let hdr_area = crate::tr!("aec", "schedule-area");
    let hdr_storey = crate::tr!("aec", "schedule-storey-id");
    table.set_cell_text(0, 0, &hdr_name);
    table.set_cell_text(0, 1, &hdr_area);
    table.set_cell_text(0, 2, &hdr_storey);
    for (row, (name, area, storey)) in rooms.iter().enumerate() {
        table.set_cell_text(row + 1, 0, name);
        table.set_cell_text(row + 1, 1, &format!("{area:.2}"));
        table.set_cell_text(row + 1, 2, &storey.to_string());
    }

    let handle = scene.add_entity(EntityType::Table(table));
    scene.bump_geometry();
    command_line.push_info(&crate::tr!(
        "aec",
        "schedule-created",
        count = rooms.len(),
        handle = handle.to_string()
    ));
}

/// `AEC_IFCEXPORT` — collect walls/rooms/storeys and emit IFC4 SPF (in-memory).
pub fn aec_ifc_export(scene: &mut Scene, command_line: &mut CommandLine) {
    let mut ifc_scene = engine::Scene::default();

    // Add in-memory storeys
    {
        let storeys = STOREYS.lock().unwrap();
        for (i, s) in storeys.iter().enumerate() {
            ifc_scene.storeys.push((i as u32, s.clone()));
        }
    }

    // Collect walls and rooms from document XDATA
    for entity in scene.document.entities() {
        let Some(record) = read_aec_record(entity) else {
            continue;
        };
        match record.values.first() {
            Some(XDataValue::String(kind)) if kind == "WALL" => {
                if let Some(wall) = wall_from_entity(entity) {
                    ifc_scene.walls.push(wall);
                }
            }
            Some(XDataValue::String(kind)) if kind == "ROOM" => {
                // 0: "ROOM", 1: name, 2: area, 3: perim, 4: vol, 5: storey_id
                if record.values.len() >= 6 {
                    let name = if let XDataValue::String(s) = &record.values[1] {
                        s.clone()
                    } else {
                        "Unknown".to_string()
                    };
                    let area = if let XDataValue::Real(r) = record.values[2] {
                        r
                    } else {
                        0.0
                    };
                    let perimeter = if let XDataValue::Real(r) = record.values[3] {
                        r
                    } else {
                        0.0
                    };
                    let volume = if let XDataValue::Real(r) = record.values[4] {
                        r
                    } else {
                        0.0
                    };
                    let storey_id = if let XDataValue::Integer32(i) = record.values[5] {
                        i as u32
                    } else {
                        0
                    };
                    ifc_scene.rooms.push(Room {
                        name,
                        area,
                        perimeter,
                        volume,
                        storey_id,
                    });
                }
            }
            _ => {}
        }
    }

    let ifc_data = engine::ifc::write_spf(&ifc_scene);
    command_line.push_info(&crate::tr!(
        "aec",
        "ifc-exported",
        bytes = ifc_data.len()
    ));
    command_line.push_info(&crate::tr!("aec", "ifc-note"));
}

/// `AEC_WALLJOIN` — interactive front-end: pick two wall entities (clicking a
/// derived contour/hatch/solid resolves to its axis, like every other wall
/// selection, via [`resolve_wall_package`]), then delegate the actual
/// geometry join to [`aec_walljoin_do`] via [`CmdResult::Dispatch`] once both
/// picks are in — the same "gather interactively, execute non-interactively
/// with full scene access" split used by [`MaterialCommand`]/`AEC_MATERIAL_ADD`.
pub struct WallJoinCommand {
    selected: Vec<Handle>,
}

impl WallJoinCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
        }
    }
}

impl CadCommand for WallJoinCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLJOIN"
    }

    fn prompt(&self) -> String {
        match self.selected.len() {
            0 => crate::tr!("aec", "walljoin-first"),
            _ => crate::tr!("aec", "walljoin-second"),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    /// Highlight wall packages for both the first and the second pick.
    fn entity_pick_highlights_hover(&self) -> bool {
        self.selected.len() < 2
    }

    /// Restrict the rollover highlight to wall packages (axis or derived).
    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    /// Walls are rendered as filled contours, so a click anywhere inside the
    /// wall's body (not just precisely on its outline) must resolve to the
    /// wall entity; otherwise clicking a wall almost always misses.
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    /// N-way-aware hover preview: while awaiting the second (target) wall,
    /// check whether joining the already-selected wall with the currently
    /// hovered candidate would actually resolve into a 3+ way junction (i.e.
    /// another wall already shares that corner). If so, highlight every OTHER
    /// participant of that prospective junction with a preview wire — the
    /// hovered handle itself is already covered by the normal single-handle
    /// hover highlight. Plain L/T (2-wall) joins keep relying solely on that
    /// single-handle highlight, unchanged.
    fn entity_pick_acquire_previews(&self, scene: &Scene, handle: Handle) -> Vec<WireModel> {
        if self.selected.len() != 1 || handle.is_null() {
            return vec![];
        }
        let axis_a = resolve_wall_package(scene, self.selected[0]);
        let axis_b = resolve_wall_package(scene, handle);
        if axis_a.is_null() || axis_b.is_null() || axis_a == axis_b {
            return vec![];
        }

        let handles = all_wall_axis_handles(scene);
        let (Some(idx_a), Some(idx_b)) = (
            handles.iter().position(|h| *h == axis_a),
            handles.iter().position(|h| *h == axis_b),
        ) else {
            return vec![];
        };
        let axes: Vec<Vec<DVec3>> = handles
            .iter()
            .map(|h| get_wall_vertices(scene, *h))
            .collect();
        let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
        let junctions = join::detect_junctions(&axis_refs, WALL_JOIN_SNAP_RADIUS);

        for junc in junctions.into_iter().filter(|j| j.is_multi_wall()) {
            let has_a = junc.participants.iter().any(|p| p.wall_index == idx_a);
            let has_b = junc.participants.iter().any(|p| p.wall_index == idx_b);
            if !has_a || !has_b {
                continue;
            }
            // Only the 3+-way case gets the extra multi-wire preview.
            let mut wires = Vec::new();
            for p in &junc.participants {
                let h = handles[p.wall_index];
                if h == axis_b {
                    continue; // already shown via the single-handle hover highlight
                }
                let pts: Vec<[f64; 3]> = axes[p.wall_index]
                    .iter()
                    .map(|v| [v.x, v.y, v.z])
                    .collect();
                if pts.len() < 2 {
                    continue;
                }
                let mut wire = WireModel::solid_f64(
                    format!("__walljoin_junction_preview_{}__", h.value()),
                    pts,
                    WireModel::HOVER,
                    false,
                );
                wire.line_weight_px = wire.line_weight_px.max(2.0);
                wires.push(wire);
            }
            return wires;
        }
        vec![]
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.selected.push(handle);
        if self.selected.len() == 2 {
            let a = self.selected[0];
            let b = self.selected[1];
            CmdResult::Dispatch(format!("AEC_WALLJOIN_DO {}|{}", a.value(), b.value()))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// True when `handle` resolves to a wall axis (or is itself a wall axis).
pub fn is_wall_pick_target(scene: &Scene, handle: Handle) -> bool {
    if handle.is_null() {
        return false;
    }
    let axis = resolve_wall_package(scene, handle);
    scene
        .document
        .get_entity(axis)
        .is_some_and(|e| wall_thickness_and_height(e).is_some())
}

/// The point `ext_len` further out from `axis[idx]`, continuing in the same
/// direction the wall's end segment already runs (i.e. straight past the
/// corner, away from the wall's interior). Used to build the corner-
/// extension hint for [`regenerate_wall_representation_with_corner`] (Bug 4).
fn extended_endpoint(axis: &[DVec3], idx: usize, ext_len: f64) -> DVec3 {
    let dir = if idx == 0 {
        (axis[0] - axis[1]).normalize_or_zero()
    } else {
        (axis[idx] - axis[idx - 1]).normalize_or_zero()
    };
    axis[idx] + dir * ext_len
}

/// Shortest 2D distance from `p` to the finite segment `a`–`b`.
fn point_to_segment_dist_2d(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = DVec3::new(b.x - a.x, b.y - a.y, 0.0);
    let ap = DVec3::new(p.x - a.x, p.y - a.y, 0.0);
    let len_sq = ab.length_squared();
    if len_sq < 1e-24 {
        return ap.length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = DVec3::new(a.x + ab.x * t, a.y + ab.y * t, 0.0);
    DVec3::new(p.x - closest.x, p.y - closest.y, 0.0).length()
}

/// Shortest 2D distance from `p` to any segment of the wall axis polyline.
fn point_to_polyline_dist_2d(p: DVec3, poly: &[DVec3]) -> f64 {
    let mut best = f64::INFINITY;
    for pair in poly.windows(2) {
        best = best.min(point_to_segment_dist_2d(p, pair[0], pair[1]));
    }
    best
}

/// True when `entity` is a wall *axis* (carries `WALL` XDATA), not a
/// derived contour/hatch/solid.
fn is_wall_axis_xdata(entity: &EntityType) -> bool {
    matches!(
        read_aec_record(entity).and_then(|r| r.values.first()),
        Some(XDataValue::String(kind)) if kind == "WALL"
    )
}

/// Minimum 2D distance between either wall's endpoints and the other wall's
/// axis polyline. Used by auto-join snap detection.
fn wall_endpoint_to_axis_dist(axis_a: &[DVec3], axis_b: &[DVec3]) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for end in [axis_a[0], *axis_a.last().unwrap()] {
        best = best.min(point_to_polyline_dist_2d(end, axis_b));
    }
    for end in [axis_b[0], *axis_b.last().unwrap()] {
        best = best.min(point_to_polyline_dist_2d(end, axis_a));
    }
    best
}

/// Drop junction overrides on ends that no longer sit next to any other wall.
fn drop_orphaned_junction_overrides(scene: &mut Scene, wall_handle: Handle) {
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return;
    }
    for (end_index, pt) in [(0usize, axis[0]), (1usize, *axis.last().unwrap())] {
        if !wall_end_has_nearby_partner(scene, wall_handle, pt) {
            remove_junction_override(scene, wall_handle, end_index);
        }
    }
}

fn wall_end_has_nearby_partner(scene: &Scene, wall_handle: Handle, pt: DVec3) -> bool {
    for entity in scene.document.entities() {
        let other = entity.common().handle;
        if other == wall_handle || !is_wall_axis_xdata(entity) {
            continue;
        }
        let other_axis = get_wall_vertices(scene, other);
        if other_axis.len() < 2 {
            continue;
        }
        if point_to_polyline_dist_2d(pt, &other_axis) <= WALL_JOIN_SNAP_RADIUS {
            return true;
        }
    }
    false
}

/// Minimum 2D distance between either wall's endpoints and the other wall's
/// endpoints only (no interior/axis-mid points). A small distance here means
/// a clean End-End (L) match; used to rank join candidates above vaguer
/// End-Mid (T) matches at a similar overall distance.
fn wall_endpoint_to_endpoint_dist(axis_a: &[DVec3], axis_b: &[DVec3]) -> f64 {
    if axis_a.len() < 2 || axis_b.len() < 2 {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for end_a in [axis_a[0], *axis_a.last().unwrap()] {
        for end_b in [axis_b[0], *axis_b.last().unwrap()] {
            best = best.min(DVec3::new(end_a.x - end_b.x, end_a.y - end_b.y, 0.0).length());
        }
    }
    best
}

/// Find the closest other wall axis whose geometry is within
/// [`WALL_JOIN_SNAP_RADIUS`] of `wall_handle`'s axis and that can actually be
/// joined (L/T intersection exists). Returns `None` when nothing is in range —
/// a graceful no-op for callers.
///
/// Candidates are ranked with a clear-endpoint priority: an End-End (L) match
/// within the snap radius always wins over a vaguer End-Mid (T) match, even
/// if the T candidate happens to be nominally closer, since an exact endpoint
/// coincidence is the more deliberate, less error-prone user intent to snap
/// against. Ties within each tier fall back to plain distance.
///
/// `excluding` skips walls already being processed (the edited wall itself —
/// this also prevents a wall from ever auto-joining to itself while it is
/// still being drawn — plus any partners already joined in the same finalize
/// pass).
pub fn find_wall_to_auto_join(
    scene: &Scene,
    wall_handle: Handle,
    excluding: &[Handle],
) -> Option<Handle> {
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return None;
    }
    // (priority tier, distance): tier 0 = clear endpoint-to-endpoint match
    // within the snap radius, tier 1 = endpoint-to-interior (T) match only.
    let mut best: Option<(Handle, u8, f64)> = None;
    for entity in scene.document.entities() {
        let other = entity.common().handle;
        if other == wall_handle || excluding.contains(&other) {
            continue;
        }
        if !is_wall_axis_xdata(entity) {
            continue;
        }
        let other_axis = get_wall_vertices(scene, other);
        if other_axis.len() < 2 {
            continue;
        }
        let dist = wall_endpoint_to_axis_dist(&axis, &other_axis);
        if dist > WALL_JOIN_SNAP_RADIUS {
            continue;
        }
        // Only accept candidates that the join engine can actually connect.
        let bulges = get_wall_bulges(scene, wall_handle);
        let other_bulges = get_wall_bulges(scene, other);
        if join::join_wall_axes_with_bulges(&axis, &bulges, &other_axis, &other_bulges).is_err() {
            continue;
        }
        let endpoint_dist = wall_endpoint_to_endpoint_dist(&axis, &other_axis);
        let tier = if endpoint_dist <= WALL_JOIN_SNAP_RADIUS {
            0
        } else {
            1
        };
        if best.map_or(true, |(_, best_tier, best_d)| {
            (tier, dist) < (best_tier, best_d)
        }) {
            best = Some((other, tier, dist));
        }
    }
    best.map(|(h, _, _)| h)
}

/// Attempt automatic L/T joins for `wall_handle` against nearby walls (up to
/// one join per endpoint). When 3+ walls meet at a shared point, resolves the
/// full junction together (N-way miter); otherwise falls back to pairwise
/// [`join_two_walls_in_document`]. Returns every axis + derived handle touched
/// so callers can refresh 2D and 3D in one `bump_entities` call. Never errors —
/// failed/no-candidate joins are silent no-ops.
pub fn try_auto_join_nearby_walls(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let mut touched = Vec::new();

    // Step 4: Symmetric peer unlinking for walls that are no longer nearby.
    // When a wall vertex is dragged away from a junction, its peer links and
    // mitered footprints must be cleaned up on both sides.
    let old_peers = engine::owner_index::peers_of(&scene.document, wall_handle);
    let axis = get_wall_vertices(scene, wall_handle);
    for peer in old_peers {
        let peer_axis = get_wall_vertices(scene, peer);
        if wall_endpoint_to_axis_dist(&axis, &peer_axis) > WALL_JOIN_SNAP_RADIUS {
            engine::owner_index::unlink_peers(&mut scene.document, wall_handle, peer);
            drop_orphaned_junction_overrides(scene, wall_handle);
            drop_orphaned_junction_overrides(scene, peer);
            // Peer representation might be mitered against us; refresh it.
            if let Ok(t) = regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                peer,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                touched.extend(t);
            }
        }
    }

    let mut excluding = vec![wall_handle];

    // Prefer multi-wall junction resolution when 3+ walls already cluster at
    // an endpoint of `wall_handle` (or a nearby through-hit).
    let junction_touched = try_join_multi_wall_junctions(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    if !junction_touched.is_empty() {
        touched.extend(junction_touched.iter().copied());
        // Walls already rebuilt via the junction path shouldn't be pairwise-
        // joined again in this pass.
        for h in &junction_touched {
            if *h != wall_handle && !excluding.contains(h) {
                // Only treat axis handles as exclusions (derived handles are
                // also in the touched list).
                if scene
                    .document
                    .get_entity(*h)
                    .is_some_and(is_wall_axis_xdata)
                {
                    excluding.push(*h);
                }
            }
        }
    }

    // Pairwise fallback for remaining simple 2-wall L/T joins (at most two:
    // start endpoint + end endpoint against different walls).
    for _ in 0..2 {
        let Some(other) = find_wall_to_auto_join(scene, wall_handle, &excluding) else {
            break;
        };
        match join_two_walls_in_document(
            scene,
            wall_handle,
            other,
            library_override,
            display_rules,
            style_substitutions,
        ) {
            Ok((_kind, handles)) => {
                touched.extend(handles);
                excluding.push(other);
            }
            Err(_) => {
                // Candidate looked joinable at search time but failed now
                // (geometry race); skip it and stop rather than looping forever.
                excluding.push(other);
            }
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// After a newly committed 2-point wall, join it to the previous chain
/// segment and any other walls within [`WALL_JOIN_SNAP_RADIUS`]. Join
/// failures are non-fatal (the new segment stays in the document).
fn auto_join_committed_wall_segment(
    scene: &mut Scene,
    wall_handle: Handle,
    last_committed: Option<Handle>,
    library: Option<&StyleLibrary>,
) {
    let axis = get_wall_vertices(scene, wall_handle);
    let ends: Vec<DVec3> = if axis.len() >= 2 {
        vec![axis[0], *axis.last().unwrap()]
    } else {
        Vec::new()
    };
    for end in ends {
        let mut cluster = vec![wall_handle];
        if let Some(prev) = last_committed {
            if prev != wall_handle {
                let prev_axis = get_wall_vertices(scene, prev);
                let prev_near = prev_axis.len() >= 2
                    && (prev_axis[0].distance(end) <= WALL_JOIN_SNAP_RADIUS
                        || prev_axis.last().unwrap().distance(end) <= WALL_JOIN_SNAP_RADIUS);
                if prev_near {
                    cluster.push(prev);
                }
            }
        }
        for h in all_wall_axis_handles(scene) {
            if h == wall_handle || cluster.contains(&h) {
                continue;
            }
            let other = get_wall_vertices(scene, h);
            if other.len() < 2 {
                continue;
            }
            let near = other[0].distance(end) <= WALL_JOIN_SNAP_RADIUS
                || other.last().unwrap().distance(end) <= WALL_JOIN_SNAP_RADIUS
                || (0..other.len() - 1).any(|i| {
                    point_to_segment_dist_2d(end, other[i], other[i + 1]) <= WALL_JOIN_SNAP_RADIUS
                });
            if near {
                cluster.push(h);
            }
        }
        cluster.sort_by_key(|h| h.value());
        cluster.dedup();
        if cluster.len() < 2 {
            continue;
        }
        match join_junction_in_document(
            scene,
            &cluster,
            Some(end),
            library,
            None,
            None,
        ) {
            Ok(_) => {}
            Err(JoinError::Ambiguous) => {
                if let Some(prev) = last_committed {
                    if let Err(err) = join_junction_in_document(
                        scene,
                        &[prev, wall_handle],
                        Some(end),
                        library,
                        None,
                        None,
                    ) {
                        queue_override_warning(format!("AEC_WALL join: {err:?}"));
                    }
                }
            }
            Err(err) => {
                queue_override_warning(format!("AEC_WALL join: {err:?}"));
            }
        }
    }
    let _ = try_auto_join_nearby_walls(scene, wall_handle, library, None, None);
}

/// Regenerates every wall in `scene` under `config`'s wall
/// [`engine::display_component::ComponentRuleSet`] (via
/// [`DisplayConfig::wall_rules`]) and `style_substitutions`. This is the
/// entry point the "active DisplayConfig" dropdown/manager (Step 5) uses
/// to apply a selected `DisplayConfig` to the whole document at once,
/// mirroring what [`regenerate_wall_representation_with_rules_and_substitutions`]
/// does for a single wall. Returns every handle touched (axis + derived),
/// same convention as [`refresh_wall_after_axis_edit`]. Walls whose
/// regeneration fails (e.g. no layers) are skipped silently, same as a
/// single-wall regeneration failure would be.
pub fn apply_display_config_to_scene(
    scene: &mut Scene,
    config: &engine::plan_view::DisplayConfig,
    library_override: Option<&StyleLibrary>,
) -> Vec<Handle> {
    apply_display_config_to_scene_with_representation(scene, config, library_override, None)
}

/// Like [`apply_display_config_to_scene`], but a session `RepresentationMode`
/// (status-bar 2D/3D/All) overrides the plan-type default.
pub fn apply_display_config_to_scene_with_representation(
    scene: &mut Scene,
    config: &engine::plan_view::DisplayConfig,
    library_override: Option<&StyleLibrary>,
    representation_override: Option<engine::display_component::RepresentationMode>,
) -> Vec<Handle> {
    // `DisplayConfig::wall_rules`/`style_substitutions` were removed in
    // Step 2 (overrides now live per-wall-style on
    // `WallStyle::display_profiles`, keyed by `DisplayConfig::name`); each
    // wall's own style is resolved against `config.name` via
    // `engine::library::resolve_effective_rule_set` below, instead of a
    // single document-wide rule set. `style_substitutions` has no
    // successor concept (removed without migration), so it's always
    // `None`. `config.phase_filter` (Step 1) hides walls whose `phase`
    // isn't in `visible_phases`; visible demolition/existing walls get an
    // additional style overlay merged on top of their resolved rules.
    let owned_lib = library_override.map_or_else(
        || engine::project::resolve_style_library(None),
        |l| l.clone(),
    );
    let library_override = Some(&owned_lib);
    let mut touched = Vec::new();
    for wall_handle in all_wall_axis_handles(scene) {
        let Some(entity) = scene.document.get_entity(wall_handle) else {
            continue;
        };
        let Some(wall) = wall_from_entity(entity) else {
            continue;
        };
        let filter_result = apply_phase_filter(wall.phase, config.phase_filter.as_ref());
        if !filter_result.visible {
            let mut hidden = engine::display_component::ComponentRuleSet::default();
            hide_all_display_slots(&mut hidden);
            if let Ok(handles) = regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                wall_handle,
                Some(&hidden),
                None,
                library_override,
            ) {
                touched.extend(handles);
            }
            continue;
        }

        let style = owned_lib
            .wall_styles
            .iter()
            .find(|ws| ws.style.id == wall.style_id);
        let mut rules = engine::library::build_effective_rule_set(
            config,
            style,
            representation_override,
        );
        // Phase extras apply only to 2D overall contour lines (fieldwise).
        if let Some(ov) = filter_result.extra_style {
            merge_phase_extra_into_rules(&mut rules, ov);
        }
        let effective_rules = Some(rules);

        if let Ok(handles) = regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            effective_rules.as_ref(),
            None,
            library_override,
        ) {
            touched.extend(handles);
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// Collect every wall axis handle in the document (excluding derived geometry).
fn all_wall_axis_handles(scene: &Scene) -> Vec<Handle> {
    scene
        .document
        .entities()
        .filter(|e| is_wall_axis_xdata(e))
        .map(|e| e.common().handle)
        .collect()
}

/// Detect multi-wall junctions involving `wall_handle` and resolve them with
/// N-way miter. Returns touched handles (empty when no multi-wall junction).
fn try_join_multi_wall_junctions(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    let handles = all_wall_axis_handles(scene);
    if handles.len() < 3 {
        return Vec::new();
    }
    let inputs: Vec<WallJoinInput> = handles
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    // Step 4: Use a larger tolerance for multi-wall junctions to ensure that
    // vertex-dragged walls still cluster with their former junction peers
    // (up to the snap radius) so the full junction is re-resolved together.
    let solved = junction_solver::solve(&inputs, WALL_JOIN_SNAP_RADIUS);

    let self_idx = handles.iter().position(|h| *h == wall_handle);
    let Some(self_idx) = self_idx else {
        return Vec::new();
    };

    let mut touched = Vec::new();
    for sj in solved.into_iter().filter(|s| s.junction.is_multi_wall()) {
        if !sj
            .junction
            .participants
            .iter()
            .any(|p| p.wall_index == self_idx)
        {
            continue;
        }
        // Restrict the participant set to walls in this junction.
        let part_handles: Vec<Handle> = sj
            .junction
            .participants
            .iter()
            .map(|p| handles[p.wall_index])
            .collect();
        // Use the moved wall's own (post-move) endpoint as the snap point so
        // the rebuilt junction lands exactly where the user dragged it,
        // rather than at the mean of all participants.
        let snap_point = sj
            .junction
            .participants
            .iter()
            .find(|p| p.wall_index == self_idx)
            .and_then(|p| match p.role {
                join::JunctionRole::Endpoint(end_idx) => inputs[self_idx].axis.get(end_idx).copied(),
                join::JunctionRole::Through(_) => None,
            });
        if let Ok(t) = join_junction_in_document(
            scene,
            &part_handles,
            snap_point,
            library_override,
            display_rules,
            style_substitutions,
        ) {
            touched.extend(t);
        }
    }
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// A single wall's participation in a junction, as discovered by
/// [`walls_at_junction`]: which axis/end it is, and its material layers
/// (outer→inner) expressed as [`join::LayerRef`]s for use in
/// [`join::LayerPairOverride`] construction.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionParticipant {
    pub axis_handle: Handle,
    pub end_index: usize,
    pub layers: Vec<join::LayerRef>,
    /// `true` when this wall does not end at the junction but merely passes
    /// through it (a T-junction's "through" wall). Such walls have no
    /// editable junction end of their own, but must still be listed so the
    /// Junction Editor shows every connected wall, not just the stem.
    pub is_through: bool,
}

/// Find every wall participating in the same junction node as
/// `(axis_handle, end_index)`, i.e. every wall whose axis endpoint (or
/// through-hit) shares the same clustered point. Reuses the same
/// [`join::detect_junctions`] topology already used by N-way join
/// resolution (see [`try_join_multi_wall_junctions`] /
/// [`join_junction_in_document`]) so the Junction-Editor-Panel and the
/// N-way join resolver always agree on who participates.
///
/// Both [`join::JunctionRole::Endpoint`] and [`join::JunctionRole::Through`]
/// participants are returned — a T-junction's "through" wall has no
/// editable junction end of its own, but must still show up in the list so
/// the user can see it is connected (see `JunctionParticipant::is_through`).
/// When no cluster is found (e.g. an isolated wall end), a single-element
/// result containing just the queried wall is returned so the caller can
/// still build a `JunctionOverride` for it.
pub fn walls_at_junction(
    scene: &Scene,
    axis_handle: Handle,
    end_index: usize,
) -> Vec<JunctionParticipant> {
    let handles = all_wall_axis_handles(scene);
    let axes: Vec<Vec<DVec3>> = handles.iter().map(|h| get_wall_vertices(scene, *h)).collect();
    let axis_refs: Vec<&[DVec3]> = axes.iter().map(|a| a.as_slice()).collect();
    let tol = join::JUNCTION_TOLERANCE.max(1e-4);
    let junctions = join::detect_junctions(&axis_refs, tol);

    let layers_for = |handle: Handle| -> Vec<join::LayerRef> {
        scene
            .document
            .get_entity(handle)
            .and_then(wall_from_entity)
            .map(|w| layer_refs_from_materials(w.layers.iter().map(|l| (l.material.as_str(), l.layer_id))))
            .unwrap_or_default()
    };

    let Some(self_idx) = handles.iter().position(|h| *h == axis_handle) else {
        return vec![JunctionParticipant {
            axis_handle,
            end_index,
            layers: layers_for(axis_handle),
            is_through: false,
        }];
    };

    // `JunctionRole::Endpoint` carries the *raw* vertex index (`0` or
    // `axis.len() - 1`, i.e. potentially > 1 for multi-segment wall axes),
    // while `end_index` (both the parameter here and every other AEC
    // junction-override API, e.g. `write_junction_override`) uses the
    // normalized `0` = start / `1` = end convention. Comparing them
    // directly would never match for walls with more than two axis points,
    // which is exactly why the editor previously fell back to "just this
    // wall" for such walls despite a real junction existing.
    let normalize_end = |raw: usize| -> usize { if raw == 0 { 0 } else { 1 } };

    for junc in &junctions {
        let matches_self = junc.participants.iter().any(|p| {
            p.wall_index == self_idx
                && matches!(p.role, join::JunctionRole::Endpoint(e) if normalize_end(e) == end_index)
        });
        if !matches_self {
            continue;
        }
        let mut out: Vec<JunctionParticipant> = junc
            .participants
            .iter()
            .map(|p| match p.role {
                join::JunctionRole::Endpoint(e) => {
                    let h = handles[p.wall_index];
                    JunctionParticipant {
                        axis_handle: h,
                        end_index: normalize_end(e),
                        layers: layers_for(h),
                        is_through: false,
                    }
                }
                join::JunctionRole::Through(_) => {
                    let h = handles[p.wall_index];
                    JunctionParticipant {
                        axis_handle: h,
                        // A through-wall has no editable end at this
                        // junction; the index is unused for it (no override
                        // lookups are ever keyed by it), just kept out of
                        // the normalized 0/1 range so it can't accidentally
                        // be mistaken for a real endpoint.
                        end_index: usize::MAX,
                        layers: layers_for(h),
                        is_through: true,
                    }
                }
            })
            .collect();
        out.sort_by_key(|p| p.axis_handle.value());
        return out;
    }

    // No cluster found — fall back to just the queried wall.
    vec![JunctionParticipant {
        axis_handle,
        end_index,
        layers: layers_for(axis_handle),
        is_through: false,
    }]
}

/// Persist a junction override (or clear it) and rebuild every wall in that
/// junction so layer-pair / default-style edits are visible immediately.
pub fn apply_junction_override_and_rebuild(
    scene: &mut Scene,
    axis_handle: Handle,
    end_index: usize,
    override_data: Option<&join::JunctionOverride>,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Vec<Handle> {
    match override_data {
        Some(ov) if ov.default_style.is_some() || !ov.layer_pairs.is_empty() => {
            write_junction_override(scene, axis_handle, end_index, ov);
        }
        _ => {
            remove_junction_override(scene, axis_handle, end_index);
        }
    }
    let participants = walls_at_junction(scene, axis_handle, end_index);
    let handles: Vec<Handle> = {
        let mut hs: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
        if !hs.contains(&axis_handle) {
            hs.push(axis_handle);
        }
        hs.sort_by_key(|h| h.value());
        hs.dedup();
        hs
    };
    let mut touched = if handles.len() >= 2 {
        join_junction_in_document(
            scene,
            &handles,
            None,
            library_override,
            display_rules,
            style_substitutions,
        )
        .unwrap_or_else(|_| {
            refresh_wall_after_axis_edit(
                scene,
                axis_handle,
                library_override,
                display_rules,
                style_substitutions,
            )
        })
    } else {
        refresh_wall_after_axis_edit(
            scene,
            axis_handle,
            library_override,
            display_rules,
            style_substitutions,
        )
    };
    for h in &handles {
        if !touched.iter().any(|t| t == h) {
            match regenerate_wall_representation_with_rules_and_substitutions(
                scene,
                *h,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(*h),
            }
        }
    }
    scene.bump_geometry();
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    touched
}

/// Snap all participants of a multi-wall junction to the shared point and
/// rebuild every endpoint wall with N-way mitered layer footprints.
///
/// `handles` are the wall axis handles participating in the junction (order
/// does not matter). Returns every touched axis + derived handle.
///
/// `snap_point`, when provided, overrides the computed junction point (which
/// is otherwise the mean of all participating endpoints) with the given exact
/// position, and widens the detection tolerance to `WALL_JOIN_SNAP_RADIUS` so
/// a vertex that was just dragged (and is therefore no longer exactly
/// coincident with its former peers) still clusters into the same junction.
pub fn join_junction_in_document(
    scene: &mut Scene,
    handles: &[Handle],
    snap_point: Option<DVec3>,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, JoinError> {
    if handles.len() < 2 {
        return Err(JoinError::Degenerate);
    }
    let inputs: Vec<WallJoinInput> = handles
        .iter()
        .map(|h| wall_join_input_from_scene(scene, *h))
        .collect();
    if inputs.iter().any(|w| w.axis.len() < 2) {
        return Err(JoinError::Degenerate);
    }
    // Use a slightly looser tol than pure geometry equality so near-miss
    // endpoints from interactive drawing still cluster (snap radius scale).
    // When a snap point is supplied (vertex-move cascade), use the larger
    // snap radius so a just-dragged endpoint still clusters with its peers.
    let tol = if snap_point.is_some() {
        WALL_JOIN_SNAP_RADIUS
    } else {
        join::JUNCTION_TOLERANCE.max(1e-4)
    };
    let solved = junction_solver::solve(&inputs, tol);
    let multi_wall: Vec<_> = solved
        .iter()
        .filter(|s| s.junction.is_multi_wall())
        .collect();
    if multi_wall.len() > 1 {
        // Only one multi-wall junction should be present in the passed handles
        // to avoid ambiguity in which one to rebuild. The caller must filter
        // handles to a single junction's participants.
        return Err(JoinError::Ambiguous);
    }
    // Prefer N-way; otherwise resolve a single 2-wall L (End-End) or T (End-Mid).
    let mut solved_j = if let Some(j) = multi_wall.into_iter().next() {
        j.clone()
    } else {
        let two: Vec<_> = solved
            .into_iter()
            .filter(|s| s.junction.participants.len() == 2)
            .collect();
        if two.len() > 1 {
            return Err(JoinError::Ambiguous);
        }
        two.into_iter().next().ok_or(JoinError::NoIntersection)?
    };
    if let Some(pt) = snap_point {
        solved_j.junction.point = pt;
        let axis_refs: Vec<&[DVec3]> = inputs.iter().map(|w| w.axis.as_slice()).collect();
        solved_j.trimmed_axes = join::apply_junction_to_axes(&axis_refs, &solved_j.junction);
        solved_j.footprints = junction_solver::footprints_for(
            &inputs,
            &solved_j.junction,
            solved_j.kind,
            &solved_j.trimmed_axes,
        );
    }
    let junc = solved_j.junction.clone();
    let snapped = solved_j.trimmed_axes.clone();
    for p in &junc.participants {
        if matches!(p.role, join::JunctionRole::Endpoint(_)) {
            update_wall_vertices(scene, handles[p.wall_index], &snapped[p.wall_index]);
        }
    }

    // Persist cleaned overrides for the participating end only; footprints
    // already came from the solver using that same per-end override.
    for (pi, p) in junc.participants.iter().enumerate() {
        if let join::JunctionRole::Endpoint(end_idx) = p.role {
            if let Some(ov) = read_junction_override(scene, handles[p.wall_index], end_idx) {
                let self_layers = &inputs[p.wall_index].layers;
                let self_refs = layer_refs_from_materials(
                    self_layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                );
                let other_refs: Vec<join::LayerRef> = junc
                    .participants
                    .iter()
                    .enumerate()
                    .filter(|(oi, _)| *oi != pi)
                    .flat_map(|(_, op)| {
                        inputs.get(op.wall_index).into_iter().flat_map(|w| {
                            layer_refs_from_materials(
                                w.layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
                            )
                        })
                    })
                    .collect();
                let _ = validate_and_persist_junction_override(
                    scene,
                    handles[p.wall_index],
                    end_idx,
                    ov,
                    &self_refs,
                    &other_refs,
                );
            }
        }
    }

    // Max thickness among participants — used for corner_override fallback.
    let thicknesses: Vec<f64> = handles
        .iter()
        .map(|h| {
            scene
                .document
                .get_entity(*h)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t)
                .unwrap_or(0.0)
        })
        .collect();
    let max_other_half = |self_i: usize| -> f64 {
        thicknesses
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self_i)
            .map(|(_, t)| t * 0.5)
            .fold(0.0_f64, f64::max)
    };

    let mut touched = Vec::new();
    for part in junc.participants.iter() {
        let h = handles[part.wall_index];
        let axis = &snapped[part.wall_index];
        let empty: Vec<Option<Vec<(f64, f64)>>> = Vec::new();
        let fps = solved_j
            .footprints
            .get(part.wall_index)
            .unwrap_or(&empty);

        let override_pt = match part.role {
            join::JunctionRole::Endpoint(end_idx) => {
                let half = max_other_half(part.wall_index);
                if half > 0.0 {
                    Some((end_idx, extended_endpoint(axis, end_idx, half)))
                } else {
                    None
                }
            }
            join::JunctionRole::Through(_) => None,
        };

        let result = regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
            scene,
            h,
            override_pt,
            fps,
            display_rules,
            style_substitutions,
            library_override,
        );
        match result {
            Ok(t) => touched.extend(t),
            Err(_) => touched.push(h),
        }
    }

    // Symmetric pairwise peer links among all junction participants.
    for i in 0..handles.len() {
        for j in (i + 1)..handles.len() {
            engine::owner_index::link_peers(&mut scene.document, handles[i], handles[j]);
        }
    }

    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

/// Collect every handle that must be bumped after a wall edit: the axis plus
/// its current `derived_handles` (contour/hatch/solid). Used when a caller
/// needs the full package without regenerating.
pub fn wall_package_handles(scene: &Scene, wall_handle: Handle) -> Vec<Handle> {
    let mut handles = vec![wall_handle];
    for child in collect_wall_display_children(scene, wall_handle) {
        if !handles.contains(&child) {
            handles.push(child);
        }
    }
    handles
}

/// Join two wall axes in the document, rebuild both representations with
/// mitered layer footprints, and return `(join_kind, every_touched_handle)`.
/// The handle set always includes both axes plus every newly created derived
/// entity so callers can refresh 2D (resident wires/hatches) and 3D (meshes)
/// together.
///
/// After the pairwise axis join, if a third (or more) wall already meets at
/// the join point the full junction is re-resolved with N-way miters so
/// pairwise overwrites cannot leave inconsistent footprints.
pub fn join_two_walls_in_document(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    join_two_walls_in_document_inner(
        scene,
        h_a,
        h_b,
        false,
        library_override,
        display_rules,
        style_substitutions,
    )
}

/// Like [`join_two_walls_in_document`], but always forms an L-corner
/// (both axes trimmed/extended to the intersection). Used by `AEC_WALLJOIN`.
pub fn join_two_walls_as_l_in_document(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    join_two_walls_in_document_inner(
        scene,
        h_a,
        h_b,
        true,
        library_override,
        display_rules,
        style_substitutions,
    )
}

fn wall_join_input_from_scene(scene: &Scene, handle: Handle) -> WallJoinInput {
    let axis = get_wall_vertices(scene, handle);
    let last = axis.len().saturating_sub(1);
    WallJoinInput {
        axis,
        bulges: get_wall_bulges(scene, handle),
        layers: wall_layer_data(scene, handle),
        override_start: read_junction_override(scene, handle, 0),
        override_end: if last == 0 {
            None
        } else {
            read_junction_override(scene, handle, last)
        },
    }
}

fn join_two_walls_in_document_inner(
    scene: &mut Scene,
    h_a: Handle,
    h_b: Handle,
    force_l: bool,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(JoinKind, Vec<Handle>), JoinError> {
    let input_a = wall_join_input_from_scene(scene, h_a);
    let input_b = wall_join_input_from_scene(scene, h_b);
    let joined = junction_solver::solve_pair(&input_a, &input_b, force_l);
    match joined {
        Ok(pair) => {
            let new_a = pair.axis_a;
            let new_b = pair.axis_b;
            let kind = pair.kind;
            let end_a = pair.end_a;
            let end_b = pair.end_b;
            let fps_a = pair.footprints_a;
            let fps_b = pair.footprints_b;
            // Corner-extension fallback (unmatched layers) plus solver
            // precomputed miters (matched layers). Persisted axis vertices stay
            // exactly as the solver trimmed them — only the visible
            // footprint geometry changes.
            let thickness_a = scene
                .document
                .get_entity(h_a)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t);
            let thickness_b = scene
                .document
                .get_entity(h_b)
                .and_then(wall_thickness_and_height)
                .map(|(t, _, _)| t);
            let override_a = end_a.and_then(|idx| {
                thickness_b.map(|t| (idx, extended_endpoint(&new_a, idx, t * 0.5)))
            });
            let override_b = end_b.and_then(|idx| {
                thickness_a.map(|t| (idx, extended_endpoint(&new_b, idx, t * 0.5)))
            });

            update_wall_vertices(scene, h_a, &new_a);
            update_wall_vertices(scene, h_b, &new_b);

            // Re-resolve the full junction (2-wall L/T or N-way) so pairwise
            // miter writes cannot overwrite each other. Extra walls already
            // meeting at the join point are included.
            let join_pt = end_a
                .map(|i| new_a[i])
                .or_else(|| end_b.map(|i| new_b[i]));
            // Forced L-corners must not go through the T/N-way classifier:
            // detect_junctions would re-open a head-wall overhang as T.
            if !force_l {
                if let Some(pt) = join_pt {
                    let mut participants = vec![h_a, h_b];
                    for entity in scene.document.entities() {
                        let h = entity.common().handle;
                        if h == h_a || h == h_b || !is_wall_axis_xdata(entity) {
                            continue;
                        }
                        let axis = get_wall_vertices(scene, h);
                        if axis.len() < 2 {
                            continue;
                        }
                        let end_hit = [axis[0], *axis.last().unwrap()]
                            .iter()
                            .any(|e| e.distance(pt) <= join::JUNCTION_TOLERANCE.max(1e-4));
                        let through_hit = (0..axis.len() - 1).any(|i| {
                            let d = point_to_segment_dist_2d(pt, axis[i], axis[i + 1]);
                            d <= join::JUNCTION_TOLERANCE.max(1e-4)
                                && axis[i].distance(pt) > join::END_MID_TOLERANCE
                                && axis[i + 1].distance(pt) > join::END_MID_TOLERANCE
                        });
                        if end_hit || through_hit {
                            participants.push(h);
                        }
                    }
                    if let Ok(touched) = join_junction_in_document(
                        scene,
                        &participants,
                        Some(pt),
                        library_override,
                        display_rules,
                        style_substitutions,
                    ) {
                        return Ok((kind, touched));
                    }
                }
            }

            // Symmetric peer links for the successful pairwise join.
            engine::owner_index::link_peers(&mut scene.document, h_a, h_b);

            let mut touched = Vec::new();

            match regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
                scene,
                h_a,
                override_a,
                &fps_a,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(h_a),
            }

            match regenerate_wall_representation_with_precomputed_miters_rules_and_substitutions(
                scene,
                h_b,
                override_b,
                &fps_b,
                display_rules,
                style_substitutions,
                library_override,
            ) {
                Ok(t) => touched.extend(t),
                Err(_) => touched.push(h_b),
            }

            touched.sort_by_key(|h| h.value());
            touched.dedup();
            Ok((kind, touched))
        }
        Err(e) => Err(e),
    }
}

/// Material-stack layers for the join-miter helper (geometry + identity).
/// Empty when `handle` isn't a wall.
fn wall_layer_data(scene: &Scene, handle: Handle) -> Vec<engine::miter::MiterLayer> {
    let Some(entity) = scene.document.get_entity(handle) else {
        return Vec::new();
    };
    if let Some(wall) = wall_from_entity(entity) {
        return wall
            .layers
            .iter()
            .map(|l| {
                engine::miter::MiterLayer::with_id(
                    l.thickness,
                    l.axis_offset,
                    l.material.clone(),
                    l.function.clone(),
                    l.layer_id,
                )
            })
            .collect();
    }
    Vec::new()
}

/// `AEC_WALLJOIN_DO handle_a|handle_b` — the non-interactive handler
/// `WallJoinCommand` dispatches to once both walls are picked; resolves each
/// pick to its wall axis, joins the two axes with [`join::join_wall_axes`],
/// writes the trimmed/extended axes back and regenerates both walls'
/// representation. Reports [`JoinError`] via the command line instead of
/// panicking.
pub fn aec_walljoin_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let parts: Vec<&str> = args.split('|').collect();
    let [a, b] = parts.as_slice() else {
        command_line.push_error(&crate::tr!("aec", "walljoin-malformed-args"));
        return;
    };
    let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        command_line.push_error(&crate::tr!("aec", "walljoin-malformed-handles"));
        return;
    };
    let h_a = resolve_wall_package(scene, Handle::new(a));
    let h_b = resolve_wall_package(scene, Handle::new(b));
    if h_a == h_b {
        command_line.push_error(&crate::tr!("aec", "walljoin-two-different"));
        return;
    }
    let is_wall = |scene: &Scene, h: Handle| {
        scene.document.get_entity(h).is_some_and(|e| {
            matches!(
                read_aec_record(e).and_then(|r| r.values.first()),
                Some(XDataValue::String(kind)) if kind == "WALL"
            )
        })
    };
    if !is_wall(scene, h_a) || !is_wall(scene, h_b) {
        command_line.push_error(&crate::tr!("aec", "walljoin-two-walls"));
        return;
    }

    match join_two_walls_as_l_in_document(
        scene,
        h_a,
        h_b,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok((_kind, touched)) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            command_line.push_info(&crate::tr!("aec", "walljoin-ok"));
            for msg in take_pending_override_warnings() {
                command_line.push_info(&msg);
            }
        }
        Err(e) => {
            command_line.push_error(&format!("AEC_WALLJOIN: {}", e));
        }
    }
}

/// Target-acquisition mode for [`WallExtendCommand`], selectable via the
/// `Point`/`Wall` command-line option once the source wall is picked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WallExtendMode {
    /// Default: extend to the intersection with another wall's axis.
    /// The wall under the cursor is highlighted like any other entity pick.
    ToWall,
    /// Extend to an explicitly-typed/snapped point. Behaves like the normal
    /// point-picking flow (with the usual point-snap preview), not an
    /// entity pick.
    ToPoint,
}

/// `AEC_WALLEXTEND` — interactive front-end: pick a wall, then either extend
/// it to another wall's axis intersection (default `ToWall` mode, reusing
/// [`join::join_wall_axes`]) or to an explicit point (`ToPoint` mode,
/// switched into via the `Point` command option and back via `Wall`).
/// Delegates the actual write to [`aec_wallextend_do`] via
/// [`CmdResult::Dispatch`].
pub struct WallExtendCommand {
    wall: Option<Handle>,
    mode: WallExtendMode,
}

impl WallExtendCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            wall: None,
            mode: WallExtendMode::ToWall,
        }
    }
}

impl CadCommand for WallExtendCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLEXTEND"
    }

    fn prompt(&self) -> String {
        if self.wall.is_none() {
            crate::tr!("aec", "wallextend-select")
        } else if self.mode == WallExtendMode::ToPoint {
            crate::tr!("aec", "wallextend-point")
        } else {
            crate::tr!("aec", "wallextend-wall")
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.wall.is_none() {
            return Vec::new();
        }
        match self.mode {
            WallExtendMode::ToWall => vec![CmdOption::new("Point", "P")],
            WallExtendMode::ToPoint => vec![CmdOption::new("Wall", "W")],
        }
    }

    fn wants_text_input(&self) -> bool {
        self.wall.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.wall.is_none() {
            return None;
        }
        let text = text.trim();
        if self.mode == WallExtendMode::ToWall && text.eq_ignore_ascii_case("p") {
            self.mode = WallExtendMode::ToPoint;
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == WallExtendMode::ToPoint && text.eq_ignore_ascii_case("w") {
            self.mode = WallExtendMode::ToWall;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        // Entity-pick for the initial wall-to-extend selection, and again
        // while in `ToWall` mode so the target wall gets the normal rollover
        // highlight. In `ToPoint` mode we fall back to plain point-picking
        // so the usual point-snap preview is shown instead.
        self.wall.is_none() || self.mode == WallExtendMode::ToWall
    }

    /// Highlight wall packages for the source pick and the target-wall pick.
    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    /// Restrict the rollover highlight to wall packages (axis or derived).
    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    /// Walls are rendered as filled contours, so a click anywhere inside the
    /// wall's body (not just precisely on its outline) must resolve to the
    /// wall entity; otherwise clicking a wall almost always misses.
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.wall.is_none() {
            if handle.is_null() {
                return CmdResult::NeedPoint;
            }
            self.wall = Some(handle);
            return CmdResult::NeedPoint;
        }
        let wall = self.wall.unwrap();
        // In `ToWall` mode a miss or a click back on the source wall itself
        // doesn't extend anything — the user must pick a *different* wall,
        // or switch to `ToPoint` mode explicitly via the option.
        if handle.is_null() || handle == wall {
            return CmdResult::NeedPoint;
        }
        // The DO handler runs resolve_wall_package so a click on a derived
        // contour/hatch/solid of another wall still joins correctly.
        CmdResult::Dispatch(format!(
            "AEC_WALLEXTEND_DO {}|WALL|{}",
            wall.value(),
            handle.value()
        ))
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Some(wall) = self.wall {
            if self.mode == WallExtendMode::ToPoint {
                return CmdResult::Dispatch(format!(
                    "AEC_WALLEXTEND_DO {}|PT|{}|{}|{}",
                    wall.value(),
                    pt.x,
                    pt.y,
                    pt.z
                ));
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLEXTEND_DO wall|PT|x|y|z` or `AEC_WALLEXTEND_DO wall|WALL|target`
/// — the non-interactive handler `WallExtendCommand` dispatches to once the
/// target is picked. Resolves the wall pick(s) to their axis, extends the
/// nearer endpoint of `wall`'s axis (to the point, or to the intersection
/// with `target`'s axis via [`join::join_wall_axes`]), writes it back and
/// regenerates the wall's representation. Reports [`JoinError`] via the
/// command line instead of panicking.
pub fn aec_wallextend_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let mut parts = args.splitn(2, '|');
    let (Some(wall_str), Some(rest)) = (parts.next(), parts.next()) else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-args"));
        return;
    };
    let Ok(wall_val) = wall_str.parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-handle"));
        return;
    };
    let wall_handle = resolve_wall_package(scene, Handle::new(wall_val));
    let mut axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        command_line.push_error(&crate::tr!("aec", "wallextend-need-axis"));
        return;
    }

    if let Some(pt_args) = rest.strip_prefix("PT|") {
        let coords: Vec<&str> = pt_args.split('|').collect();
        let [x, y, z] = coords.as_slice() else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-point"));
            return;
        };
        let (Ok(x), Ok(y), Ok(z)) = (x.parse::<f64>(), y.parse::<f64>(), z.parse::<f64>()) else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-point"));
            return;
        };
        let pt = DVec3::new(x, y, z);
        let d1 = axis[0].distance(pt);
        let d2 = axis.last().unwrap().distance(pt);
        // Project the picked point onto the wall's existing direction line
        // instead of using it directly as the new endpoint — this preserves
        // the wall's original direction exactly, even if `pt` isn't
        // perfectly collinear (e.g. a slightly imprecise pick).
        //
        // The direction must be taken from the segment immediately adjacent
        // to the endpoint being moved (i.e. the endpoint and its neighbour),
        // NOT from the endpoint and the opposite end of the whole axis: for
        // multi-vertex wall polylines (more than two vertices) those differ,
        // and anchoring on the far end would bend the extended segment away
        // from its actual direction.
        if d1 < d2 {
            let anchor = axis[1];
            let near = axis[0];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[0] = anchor + dir * t;
            } else {
                axis[0] = pt;
            }
        } else {
            let last = axis.len() - 1;
            let anchor = axis[last - 1];
            let near = axis[last];
            let dir = near - anchor;
            if dir.length_squared() > 1e-12 {
                let t = (pt - anchor).dot(dir) / dir.length_squared();
                axis[last] = anchor + dir * t;
            } else {
                axis[last] = pt;
            }
        }
        update_wall_vertices(scene, wall_handle, &axis);
        let touched = match regenerate_wall_representation_with_rules_and_substitutions(
            scene,
            wall_handle,
            display_rules,
            style_substitutions,
            library_override,
        ) {
            Ok(t) => t,
            Err(_) => vec![wall_handle],
        };
        // Do not auto-join to a corner after a length-only extend.
        let changes: Vec<_> = touched
            .into_iter()
            .filter(|h| scene.document.get_entity(*h).is_some())
            .map(|handle| (handle, crate::scene::ChangeKind::Modified))
            .collect();
        if !changes.is_empty() {
            scene.bump_entities(&changes);
        }
        command_line.push_info(&crate::tr!("aec", "wallextend-ok"));
    } else if let Some(target_str) = rest.strip_prefix("WALL|") {
        let Ok(target_val) = target_str.parse::<u64>() else {
            command_line.push_error(&crate::tr!("aec", "wallextend-malformed-target"));
            return;
        };
        let target_handle = resolve_wall_package(scene, Handle::new(target_val));
        if target_handle == wall_handle {
            command_line.push_error(&crate::tr!("aec", "wallextend-different"));
            return;
        }
        let target_axis = get_wall_vertices(scene, target_handle);
        match join::extend_axis_to_other(&axis, &target_axis) {
            Ok((new_axis, end_idx, _isect)) => {
                update_wall_vertices(scene, wall_handle, &new_axis);
                let layers_target = wall_layer_data(scene, target_handle);
                let axis_target_2d: Vec<(f64, f64)> =
                    target_axis.iter().map(|p| (p.x, p.y)).collect();
                let miter = engine::miter::JoinMiterContext {
                    self_end: end_idx,
                    other_axis: axis_target_2d,
                    other_layers: layers_target,
                    other_end: None,
                    kind: JoinKind::T,
                    self_bulges: get_wall_bulges(scene, wall_handle),
                    other_bulges: get_wall_bulges(scene, target_handle),
                    as_through: false,
                };
                let mut touched = match regenerate_wall_representation_with_corner_rules_and_substitutions(
                    scene,
                    wall_handle,
                    None,
                    Some(&miter),
                    display_rules,
                    style_substitutions,
                    library_override,
                ) {
                    Ok(t) => t,
                    Err(_) => vec![wall_handle],
                };
                // Target length stays unchanged; refresh its display so
                // overlapping layer edges stay in sync visually.
                match regenerate_wall_representation_with_rules_and_substitutions(
                    scene,
                    target_handle,
                    display_rules,
                    style_substitutions,
                    library_override,
                ) {
                    Ok(t) => touched.extend(t),
                    Err(_) => touched.push(target_handle),
                }
                // Register the two walls as joined peers — same bookkeeping
                // `AEC_WALLJOIN`/auto-join perform after a successful join —
                // so later moves/regenerations recognize and preserve this
                // connection instead of silently treating it as unjoined.
                engine::owner_index::link_peers(&mut scene.document, wall_handle, target_handle);

                touched.sort_by_key(|h| h.value());
                touched.dedup();
                let changes: Vec<_> = touched
                    .into_iter()
                    .filter(|h| scene.document.get_entity(*h).is_some())
                    .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                    .collect();
                if !changes.is_empty() {
                    scene.bump_entities(&changes);
                }
                command_line.push_info(&crate::tr!("aec", "wallextend-to-wall-ok"));
            }
            Err(e) => {
                command_line.push_error(&format!("AEC_WALLEXTEND: {}", e));
            }
        }
    } else {
        command_line.push_error(&crate::tr!("aec", "wallextend-malformed-args"));
    }
}

/// `AEC_WALLREVERSE` — interactive front-end: pick one wall entity (derived
/// contour/hatch/solid resolves to its axis via [`resolve_wall_package`]),
/// then reverse its axis direction and layer-stack side assignment.
pub struct WallReverseCommand {
    done: bool,
}

impl WallReverseCommand {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { done: false }
    }
}

impl CadCommand for WallReverseCommand {
    fn name(&self) -> &'static str {
        "AEC_WALLREVERSE"
    }

    fn prompt(&self) -> String {
        crate::tr!("aec", "wallreverse-select")
    }

    fn needs_entity_pick(&self) -> bool {
        !self.done
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        !self.done
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    /// Walls are rendered as filled contours, so a click anywhere inside the
    /// wall's body (not just precisely on its outline) must resolve to the
    /// wall entity; otherwise clicking a wall almost always misses.
    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.done = true;
        CmdResult::Dispatch(format!("AEC_WALLREVERSE_DO {}", handle.value()))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Reverse a wall's axis vertex order and mirror its layer stack so the
/// absolute visible footprint (including which material sits on which world
/// side) stays pixel-identical while start/end and left/right-relative-to-
/// direction flip. Regenerates the representation and re-runs auto-join.
///
/// Returns every axis + derived handle touched (including any auto-joined
/// neighbours) so callers can bump 2D/3D together.
pub fn reverse_wall_in_document(
    scene: &mut Scene,
    wall_handle: Handle,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<Vec<Handle>, WallRegenError> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return Err(WallRegenError::NotAWall);
    }

    // Capture pre-reverse layer-contour outer bounds for callers/tests that
    // want to assert footprint stability; the reverse itself only needs the
    // axis + layer list transform below.
    axis.reverse();
    update_wall_vertices(scene, wall_handle, &axis);

    // NOTE: the axis-direction flip above already inverts the offset normal
    // used by `layer_contours`, which on its own physically swaps every
    // layer to the opposite absolute side of the wall (this is the intended,
    // visible effect of "reverse direction" — same footprint, materials
    // swapped). We must NOT also mirror/reverse the stored layer list here:
    // doing so cancels the normal flip exactly, leaving the wall completely
    // unchanged (a previous bug). Interior/Exterior justification still
    // swaps with the direction since "interior"/"exterior" is direction-
    // relative.
    if let Some(entity) = scene.document.get_entity(wall_handle) {
        if let Some(mut v2) = wall_from_entity(entity) {
            v2.justification = match v2.justification {
                WallJustification::Interior => WallJustification::Exterior,
                WallJustification::Exterior => WallJustification::Interior,
                WallJustification::Center => WallJustification::Center,
            };
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            for v in wall_record(
                &v2.style_id,
                v2.height,
                v2.storey_id,
                &v2.layers,
                &v2.derived_handles,
                v2.justification, v2.phase, v2.hatch_override.as_ref()) {
                record.add_value(v);
            }
            write_aec_record(&mut scene.document, wall_handle, record);
        }
    }

    let mut touched = regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    )?;
    let joined = try_auto_join_nearby_walls(
        scene,
        wall_handle,
        library_override,
        display_rules,
        style_substitutions,
    );
    touched.extend(joined);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

/// `AEC_WALLREVERSE_DO handle` — non-interactive handler dispatched by
/// [`WallReverseCommand`] once a wall is picked.
pub fn aec_wallreverse_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let Ok(val) = args.trim().parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "wallreverse-malformed"));
        return;
    };
    let handle = resolve_wall_package(scene, Handle::new(val));
    if !is_wall_pick_target(scene, handle) {
        command_line.push_error(&crate::tr!("aec", "wallreverse-need-wall"));
        return;
    }
    match reverse_wall_in_document(
        scene,
        handle,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok(touched) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            command_line.push_info(&crate::tr!("aec", "wallreverse-ok"));
        }
        Err(e) => {
            command_line.push_error(&format!("AEC_WALLREVERSE: {e:?}"));
        }
    }
}

// ── Wall openings (window / door) ──────────────────────────────────────────

/// Build an `OPENING` XDATA record for a standalone opening entity.
fn opening_record(opening: &engine::openings::Opening) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String("OPENING".to_string()));
    record.add_value(XDataValue::Handle(opening.host_wall));
    record.add_value(XDataValue::Distance(opening.distance_along_axis));
    record.add_value(XDataValue::Distance(opening.width));
    record.add_value(XDataValue::Distance(opening.height));
    record.add_value(XDataValue::Distance(opening.sill_height));
    record.add_value(XDataValue::String(opening.kind.as_str().to_string()));
    record
}

/// Parse an `OPENING` XDATA record. `handle` is the entity that carries it.
pub fn opening_from_entity(entity: &EntityType, handle: Handle) -> Option<engine::openings::Opening> {
    let record = read_aec_record(entity)?;
    let v = &record.values;
    if v.len() < 7 {
        return None;
    }
    let XDataValue::String(kind) = &v[0] else {
        return None;
    };
    if kind != "OPENING" {
        return None;
    }
    let host_wall = match &v[1] {
        XDataValue::Handle(h) => *h,
        _ => return None,
    };
    let distance_along_axis = match v[2] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let width = match v[3] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let height = match v[4] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let sill_height = match v[5] {
        XDataValue::Distance(d) => d,
        _ => return None,
    };
    let opening_kind = match &v[6] {
        XDataValue::String(s) => engine::openings::OpeningKind::from_str(s),
        _ => engine::openings::OpeningKind::Window,
    };
    Some(engine::openings::Opening {
        handle,
        host_wall,
        distance_along_axis,
        width,
        height,
        sill_height,
        kind: opening_kind,
    })
}

/// All openings whose host wall is `wall_handle`, looked up via the wall's
/// `CHILD_HANDLES` owner index (no document scan).
pub fn openings_for_host_wall(scene: &Scene, wall_handle: Handle) -> Vec<engine::openings::Opening> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    let mut out = Vec::new();
    for child in engine::owner_index::children_of(&scene.document, wall_handle) {
        let Some(entity) = scene.document.get_entity(child) else {
            continue;
        };
        if let Some(o) = opening_from_entity(entity, child) {
            out.push(o);
        }
    }
    out
}

/// Place a window/door opening on `wall_handle` at world point `pt`.
///
/// Creates a POINT entity carrying `OPENING` XDATA and regenerates the host
/// wall's 2D representation so the opening cut appears. Returns the new
/// opening handle plus every wall-derived handle touched.
pub fn place_wall_opening(
    scene: &mut Scene,
    wall_handle: Handle,
    pt: DVec3,
    kind: engine::openings::OpeningKind,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) -> Result<(Handle, Vec<Handle>), String> {
    let wall_handle = resolve_wall_package(scene, wall_handle);
    if !is_wall_pick_target(scene, wall_handle) {
        return Err("select a wall entity".into());
    }
    let axis = get_wall_vertices(scene, wall_handle);
    if axis.len() < 2 {
        return Err("wall axis is degenerate".into());
    }
    let axis_2d: Vec<(f64, f64)> = axis.iter().map(|v| (v.x, v.y)).collect();
    let distance = engine::openings::distance_along_axis_from_point(&axis_2d, (pt.x, pt.y))
        .ok_or_else(|| "could not project point onto wall axis".to_string())?;

    let placeholder = engine::openings::Opening {
        handle: Handle::NULL,
        host_wall: wall_handle,
        distance_along_axis: distance,
        width: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_WIDTH,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_WIDTH,
        },
        height: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_HEIGHT,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_HEIGHT,
        },
        sill_height: match kind {
            engine::openings::OpeningKind::Window => engine::openings::DEFAULT_WINDOW_SILL,
            engine::openings::OpeningKind::Door => engine::openings::DEFAULT_DOOR_SILL,
        },
        kind,
    };

    // Anchor the POINT at the projected axis location (not the raw click).
    let (anchor, _) = engine::openings::point_and_tangent_at_distance(&axis_2d, distance)
        .unwrap_or(((pt.x, pt.y), (1.0, 0.0)));
    let point_entity = EntityType::Point(Point::at(Vector3::new(anchor.0, anchor.1, 0.0)));
    let opening_handle = scene.add_entity(point_entity);

    let mut opening = placeholder;
    opening.handle = opening_handle;
    write_aec_record(&mut scene.document, opening_handle, opening_record(&opening));
    engine::owner_index::add_child(&mut scene.document, wall_handle, opening_handle);

    let mut touched = match regenerate_wall_representation_with_rules_and_substitutions(
        scene,
        wall_handle,
        display_rules,
        style_substitutions,
        library_override,
    ) {
        Ok(t) => t,
        Err(_) => vec![wall_handle],
    };
    touched.push(opening_handle);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok((opening_handle, touched))
}

/// Remove an opening entity, drop it from the host wall's `CHILD_HANDLES`
/// index, and regenerate the host wall. Returns touched handles (wall +
/// former opening). No-op error when `opening_handle` is not an opening.
pub fn remove_wall_opening(
    scene: &mut Scene,
    opening_handle: Handle,
    library_override: Option<&StyleLibrary>,
) -> Result<Vec<Handle>, String> {
    let Some(entity) = scene.document.get_entity(opening_handle).cloned() else {
        return Err("opening entity not found".into());
    };
    let Some(opening) = opening_from_entity(&entity, opening_handle) else {
        return Err("entity is not an opening".into());
    };
    let wall_handle = resolve_wall_package(scene, opening.host_wall);
    engine::owner_index::remove_child(&mut scene.document, wall_handle, opening_handle);
    scene.erase_entities(&[opening_handle]);
    let mut touched = match regenerate_wall_representation(scene, wall_handle, library_override) {
        Ok(t) => t,
        Err(_) => vec![wall_handle],
    };
    touched.push(opening_handle);
    touched.sort_by_key(|h| h.value());
    touched.dedup();
    Ok(touched)
}

/// `AEC_WINDOW` / `AEC_DOOR` — pick a wall, then a point along it to place
/// an opening with default dimensions.
pub struct WallOpeningCommand {
    kind: engine::openings::OpeningKind,
    wall: Option<Handle>,
}

impl WallOpeningCommand {
    #[allow(clippy::new_without_default)]
    pub fn new_window() -> Self {
        Self {
            kind: engine::openings::OpeningKind::Window,
            wall: None,
        }
    }

    #[allow(clippy::new_without_default)]
    pub fn new_door() -> Self {
        Self {
            kind: engine::openings::OpeningKind::Door,
            wall: None,
        }
    }
}

impl CadCommand for WallOpeningCommand {
    fn name(&self) -> &'static str {
        match self.kind {
            engine::openings::OpeningKind::Window => "AEC_WINDOW",
            engine::openings::OpeningKind::Door => "AEC_DOOR",
        }
    }

    fn prompt(&self) -> String {
        let tag = self.name();
        if self.wall.is_none() {
            crate::tr!("aec", "opening-select-wall", cmd = tag)
        } else {
            crate::tr!("aec", "opening-specify-point", cmd = tag)
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.wall.is_none()
    }

    fn entity_pick_hover_highlights_handle(&self, scene: &Scene, handle: Handle) -> bool {
        is_wall_pick_target(scene, handle)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.wall = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(wall) = self.wall else {
            return CmdResult::NeedPoint;
        };
        let kind_flag = match self.kind {
            engine::openings::OpeningKind::Window => "W",
            engine::openings::OpeningKind::Door => "D",
        };
        CmdResult::Dispatch(format!(
            "AEC_WALLOPENING_DO {}|{}|{},{},{}",
            wall.value(),
            kind_flag,
            pt.x,
            pt.y,
            pt.z
        ))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `AEC_WALLOPENING_DO handle|W|x,y,z` or `...|D|x,y,z` — non-interactive
/// handler dispatched by [`WallOpeningCommand`].
pub fn aec_wallopening_do(
    scene: &mut Scene,
    command_line: &mut CommandLine,
    args: &str,
    library_override: Option<&StyleLibrary>,
    display_rules: Option<&engine::display_component::ComponentRuleSet>,
    style_substitutions: Option<&HashMap<engine::plan_view::WallStyleRef, engine::plan_view::WallStyleRef>>,
) {
    let parts: Vec<&str> = args.split('|').collect();
    if parts.len() != 3 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-args"));
        return;
    }
    let Ok(wall_val) = parts[0].parse::<u64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-handle"));
        return;
    };
    let kind = match parts[1] {
        "D" | "d" | "Door" | "door" => engine::openings::OpeningKind::Door,
        _ => engine::openings::OpeningKind::Window,
    };
    let xyz: Vec<&str> = parts[2].split(',').collect();
    if xyz.len() < 2 {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    }
    let Ok(x) = xyz[0].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let Ok(y) = xyz[1].parse::<f64>() else {
        command_line.push_error(&crate::tr!("aec", "opening-malformed-point"));
        return;
    };
    let z = xyz.get(2).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);

    match place_wall_opening(
        scene,
        Handle::new(wall_val),
        DVec3::new(x, y, z),
        kind,
        library_override,
        display_rules,
        style_substitutions,
    ) {
        Ok((_opening, touched)) => {
            let changes: Vec<_> = touched
                .into_iter()
                .map(|h| (h, crate::scene::ChangeKind::Modified))
                .collect();
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
            let label = kind.as_str();
            command_line.push_info(&crate::tr!("aec", "opening-placed", kind = label));
        }
        Err(e) => {
            command_line.push_error(&crate::tr!("aec", "opening-error", error = e.to_string()));
        }
    }
}

#[cfg(test)]
mod wall_command_tests {
    use super::*;
    use acadrust::Handle;
    use glam::DVec3;

    fn wall_xdata(entity: &EntityType) -> Option<&ExtendedDataRecord> {
        read_aec_record(entity)
    }

    fn wl(material: &str, thickness: f64, function: &str) -> WallLayer {
        WallLayer {
            material: material.to_string(),
            thickness,
            function: function.to_string(),
            axis_offset: -thickness * 0.5,
            bottom_offset: 0.0,
            top_offset: 0.0,
            layer_override: None,
            hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
        }
    }

    /// Build a multi-layer stack with centered absolute axis offsets.
    fn wls(specs: &[(&str, f64, &str)]) -> Vec<WallLayer> {
        let pairs: Vec<(f64, f64)> = specs.iter().map(|(_, t, _)| (*t, 0.0)).collect();
        let offsets = migrate_gap_before_to_axis_offset(&pairs);
        specs
            .iter()
            .zip(offsets)
            .map(|(&(mat, t, fun), off)| WallLayer {
                material: mat.to_string(),
                thickness: t,
                function: fun.to_string(),
                axis_offset: off,
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: match mat {
                    "Putz" => Some("DOTS".to_string()),
                    "Mauerwerk" => Some("ANSI31".to_string()),
                    _ => None,
                },
            layer_id: uuid::Uuid::new_v4(),
            })
            .collect()
    }

    #[test]
    fn junction_override_write_read_roundtrip() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);

        let ov = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Miter),
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: Some("Tragschale".to_string()),
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::OuterFace,
            }],
        };

        assert!(write_junction_override(&mut scene, wall, 1, &ov));
        let read_back = read_junction_override(&scene, wall, 1);
        assert_eq!(read_back, Some(ov));

        // The other end of the same axis was not touched.
        assert_eq!(read_junction_override(&scene, wall, 0), None);
    }

    #[test]
    fn junction_override_missing_returns_none() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        // No override ever written on this axis.
        assert_eq!(read_junction_override(&scene, wall, 0), None);
        assert_eq!(read_junction_override(&scene, wall, 1), None);
    }

    #[test]
    fn junction_override_absent_on_old_format_entity_is_backward_compatible() {
        // Simulate a drawing saved before this feature existed: the wall
        // axis has its normal `WALL` XDATA but never the new `JOIN_OVERRIDE`
        // tag. Reading must not panic and must simply report `None`.
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let entity = scene.document.get_entity(wall).unwrap();
        assert!(wall_from_entity(entity).is_some(), "old-format wall still loads");
        assert_eq!(read_junction_override(&scene, wall, 0), None);
        assert_eq!(read_junction_override(&scene, wall, 1), None);
    }

    /// Mirrors the `Message::WallJunctionOverrideSetStyle` handler in
    /// `app/update/mod.rs`: read the existing override (if any), set
    /// `default_style`, write it back, then trigger the same immediate
    /// regeneration the context-menu action performs.
    #[test]
    fn wall_junction_context_menu_set_style_persists_and_regenerates() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 1usize;

        let mut override_data =
            read_junction_override(&scene, wall, end_index).unwrap_or_default();
        override_data.default_style = Some(join::JoinOverrideStyle::Butt);
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
        let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
        assert!(!touched.is_empty(), "regeneration should touch at least the axis");

        let read_back = read_junction_override(&scene, wall, end_index);
        assert_eq!(
            read_back.and_then(|ov| ov.default_style),
            Some(join::JoinOverrideStyle::Butt)
        );
    }

    /// Mirrors the `Message::WallJunctionOverrideReset` handler: remove the
    /// override for the junction and regenerate. `read_junction_override`
    /// must report `None` afterward.
    #[test]
    fn wall_junction_context_menu_reset_removes_override_and_regenerates() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 0usize;

        let ov = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Miter),
            layer_pairs: vec![],
        };
        assert!(write_junction_override(&mut scene, wall, end_index, &ov));
        assert!(read_junction_override(&scene, wall, end_index).is_some());

        assert!(remove_junction_override(&mut scene, wall, end_index));
        let touched = refresh_wall_after_axis_edit(&mut scene, wall, None, None, None);
        assert!(!touched.is_empty());

        assert_eq!(read_junction_override(&scene, wall, end_index), None);
    }

    fn add_wall_2layer(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64), mat: &str) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    /// Same as [`add_wall_2layer`], but with a *bent*, multi-segment axis
    /// (three vertices) so its "last vertex" raw index is `2`, not `1` — the
    /// exact shape needed to reproduce the reported bug where the Junction
    /// Editor's endpoint-index comparison broke for non-2-point wall axes.
    fn add_bent_wall_2layer(
        scene: &mut Scene,
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
        mat: &str,
    ) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p3.0, p3.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wls(&[(mat, 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    /// Regression test for the reported bug: the Junction Editor's master
    /// list ("Beteiligte W\u{e4}nde") showed only the single clicked wall even
    /// though the Properties panel correctly showed the connected walls. Root
    /// cause: `walls_at_junction` compared the raw `JunctionRole::Endpoint`
    /// vertex index (which is `axis.len() - 1` for the "far" end, e.g. `2`
    /// for a 3-vertex bent wall) directly against the normalized `0`/`1`
    /// `end_index` convention used everywhere else, so the match always
    /// failed for walls with more than two axis points, and the function
    /// fell back to returning just the queried wall.
    #[test]
    fn walls_at_junction_finds_all_participants_for_bent_multi_segment_wall() {
        let mut scene = Scene::new();
        // w1's axis has 3 vertices; its junction end is the *last* vertex,
        // whose raw index is 2 (not 1).
        let w1 = add_bent_wall_2layer(&mut scene, (-10.0, 5.0), (-10.0, 0.0), (0.0, 0.0), "Brick");
        let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
        let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
        for h in [w1, w2, w3] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

        let w1_vertices = get_wall_vertices(&scene, w1);
        let end_1 = if w1_vertices[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
        let participants = walls_at_junction(&scene, w1, end_1);

        let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
        handles.sort_by_key(|h| h.value());
        let mut expected = vec![w1, w2, w3];
        expected.sort_by_key(|h| h.value());
        assert_eq!(
            handles, expected,
            "expected all 3 walls at the junction, got {participants:?}"
        );
    }

    /// Regression test for the reported bug: a T-junction between two walls
    /// (one wall's endpoint touches the other wall's *interior*, i.e. a
    /// [`join::JunctionRole::Through`] participant) must still list the
    /// through-running wall in the Junction Editor's participating-walls
    /// list, not just the stem wall that was clicked to open the editor.
    #[test]
    fn walls_at_junction_includes_through_wall_at_t_junction() {
        let mut scene = Scene::new();
        let through = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
        let stem = add_wall_2layer(&mut scene, (5.0, 0.0), (5.0, 5.0), "Concrete");
        for h in [through, stem] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene, &[through, stem], None, None, None, None).expect("T join");

        // The stem wall's end at (5,0) is a real Endpoint; use it as the
        // query, exactly as the context menu does when the user clicks the
        // stem wall to open the Junction Editor.
        let stem_vertices = get_wall_vertices(&scene, stem);
        let stem_end = if stem_vertices[0].distance(DVec3::new(5.0, 0.0, 0.0)) < 1e-6 { 0 } else { 1 };
        let participants = walls_at_junction(&scene, stem, stem_end);

        let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
        handles.sort_by_key(|h| h.value());
        let mut expected = vec![through, stem];
        expected.sort_by_key(|h| h.value());
        assert_eq!(
            handles, expected,
            "the through wall must be listed alongside the stem wall, got {participants:?}"
        );

        let through_participant = participants
            .iter()
            .find(|p| p.axis_handle == through)
            .expect("through wall must be present");
        assert!(
            through_participant.is_through,
            "through wall participant must be flagged as is_through"
        );
        let stem_participant = participants
            .iter()
            .find(|p| p.axis_handle == stem)
            .expect("stem wall must be present");
        assert!(!stem_participant.is_through, "stem wall must not be flagged as through");
    }

    #[test]
    fn disconnecting_one_end_drops_only_that_junction_override() {
        let mut scene = Scene::new();
        let mid = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Brick");
        let left = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 5.0), "Concrete");
        let right = add_wall_2layer(&mut scene, (10.0, 0.0), (10.0, 5.0), "Wood");
        for h in [mid, left, right] {
            regenerate_wall_representation(&mut scene, h, None).expect("regen");
        }
        join_two_walls_in_document(&mut scene, mid, left, None, None, None).expect("join left");
        join_two_walls_in_document(&mut scene, mid, right, None, None, None).expect("join right");
        let ov = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Butt),
            layer_pairs: vec![],
        };
        assert!(write_junction_override(&mut scene, mid, 0, &ov));
        assert!(write_junction_override(&mut scene, mid, 1, &ov));
        update_wall_vertices(
            &mut scene,
            left,
            &[
                glam::DVec3::new(0.0, 20.0, 0.0),
                glam::DVec3::new(0.0, 25.0, 0.0),
            ],
        );
        let _ = try_auto_join_nearby_walls(&mut scene, left, None, None, None);
        assert_eq!(
            read_junction_override(&scene, mid, 0),
            None,
            "disconnected end must drop its override"
        );
        assert!(
            read_junction_override(&scene, mid, 1).is_some(),
            "the still-joined end must keep its override"
        );
    }

    /// Step 5 test 1: discovering "all walls at a junction" for a known
    /// multi-wall N-way fixture returns every participating wall handle plus
    /// its layer material ids, reusing [`join::detect_junctions`] topology.
    #[test]
    fn walls_at_junction_finds_all_n_way_participants() {
        let mut scene = Scene::new();
        let w1 = add_wall_2layer(&mut scene, (0.0, 0.0), (-10.0, 0.0), "Brick");
        let w2 = add_wall_2layer(&mut scene, (0.0, 0.0), (0.0, 10.0), "Concrete");
        let w3 = add_wall_2layer(&mut scene, (0.0, 0.0), (10.0, 0.0), "Wood");
        for h in [w1, w2, w3] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

        let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 { 0 } else { 1 };
        let participants = walls_at_junction(&scene, w1, end_1);

        let mut handles: Vec<Handle> = participants.iter().map(|p| p.axis_handle).collect();
        handles.sort_by_key(|h| h.value());
        let mut expected = vec![w1, w2, w3];
        expected.sort_by_key(|h| h.value());
        assert_eq!(handles, expected);

        let mut mats: Vec<String> = participants
            .iter()
            .flat_map(|p| p.layers.iter().map(|l| l.material_id.clone()))
            .collect();
        mats.sort();
        let mut expected_mats = vec![
            "Brick".to_string(),
            "Insulation".to_string(),
            "Concrete".to_string(),
            "Insulation".to_string(),
            "Wood".to_string(),
            "Insulation".to_string(),
        ];
        expected_mats.sort();
        assert_eq!(mats, expected_mats);
    }

    /// Step 5 test 2: adding a `LayerPairOverride` via the panel's save logic
    /// (read-mutate-write the same `JunctionOverride`) persists correctly and
    /// coexists with an existing `default_style`.
    #[test]
    fn junction_editor_adds_layer_pair_and_keeps_default_style() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 1usize;

        let mut override_data =
            read_junction_override(&scene, wall, end_index).unwrap_or_default();
        override_data.default_style = Some(join::JoinOverrideStyle::Miter);
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

        // Panel save logic: read-modify-write the same structure to add a
        // layer-pair override.
        let mut override_data =
            read_junction_override(&scene, wall, end_index).unwrap_or_default();
        override_data.layer_pairs.push(join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            },
            layer_b: None,
            style: join::JoinOverrideStyle::Butt,
        });
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

        let read_back = read_junction_override(&scene, wall, end_index).unwrap();
        assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Miter));
        assert_eq!(read_back.layer_pairs.len(), 1);
        assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::Butt);
    }

    /// Step 5 test 3: removing a single layer-pair entry (panel's per-pair
    /// "Zuruecksetzen") leaves other pairs and `default_style` intact.
    #[test]
    fn junction_editor_removes_single_layer_pair_only() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 1usize;

        let override_data = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::OuterFace),
            layer_pairs: vec![
                join::LayerPairOverride {
                    layer_a: join::LayerRef {
                        material_id: "Brick".to_string(),
                        role_tag: None,
                        index: 0,
                    layer_id: None,
                    },
                    layer_b: None,
                    style: join::JoinOverrideStyle::Miter,
                },
                join::LayerPairOverride {
                    layer_a: join::LayerRef {
                        material_id: "Insulation".to_string(),
                        role_tag: None,
                        index: 1,
                    layer_id: None,
                    },
                    layer_b: None,
                    style: join::JoinOverrideStyle::Butt,
                },
            ],
        };
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

        // Panel per-pair reset: remove only the targeted entry.
        let mut current = read_junction_override(&scene, wall, end_index).unwrap();
        current.layer_pairs.retain(|p| p.layer_a.material_id != "Brick");
        assert!(write_junction_override(&mut scene, wall, end_index, &current));

        let read_back = read_junction_override(&scene, wall, end_index).unwrap();
        assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::OuterFace));
        assert_eq!(read_back.layer_pairs.len(), 1);
        assert_eq!(read_back.layer_pairs[0].layer_a.material_id, "Insulation");
    }

    /// Step 5 test 4: a full reset via the panel removes the entire override,
    /// matching Step 4's context-menu reset exactly (same helper, same result).
    #[test]
    fn junction_editor_full_reset_matches_context_menu_reset() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 1usize;

        let override_data = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Miter),
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Brick".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::Butt,
            }],
        };
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));
        assert!(read_junction_override(&scene, wall, end_index).is_some());

        assert!(remove_junction_override(&mut scene, wall, end_index));
        assert_eq!(read_junction_override(&scene, wall, end_index), None);
    }

    /// Step 5 test 5 (consistency): setting `default_style` via the Step 4
    /// context-menu code path, then editing `layer_pairs` via the panel's
    /// code path on the SAME `(axis_handle, end_index)`, must combine both
    /// additively in the final `JunctionOverride` (no clobbering).
    #[test]
    fn context_menu_and_junction_editor_paths_combine_additively() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let end_index = 1usize;

        // Step 4 context-menu path.
        let mut override_data =
            read_junction_override(&scene, wall, end_index).unwrap_or_default();
        override_data.default_style = Some(join::JoinOverrideStyle::Butt);
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

        // Step 5 panel path, on the exact same (axis_handle, end_index).
        let mut override_data =
            read_junction_override(&scene, wall, end_index).unwrap_or_default();
        override_data.layer_pairs.push(join::LayerPairOverride {
            layer_a: join::LayerRef {
                material_id: "Insulation".to_string(),
                role_tag: None,
                index: 1,
            layer_id: None,
            },
            layer_b: Some(join::LayerRef {
                material_id: "Concrete".to_string(),
                role_tag: None,
                index: 0,
            layer_id: None,
            }),
            style: join::JoinOverrideStyle::OuterFace,
        });
        assert!(write_junction_override(&mut scene, wall, end_index, &override_data));

        let read_back = read_junction_override(&scene, wall, end_index).unwrap();
        assert_eq!(read_back.default_style, Some(join::JoinOverrideStyle::Butt));
        assert_eq!(read_back.layer_pairs.len(), 1);
        assert_eq!(read_back.layer_pairs[0].style, join::JoinOverrideStyle::OuterFace);
    }

    #[test]
    fn first_point_only_waits_for_the_next_one() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
    }

    #[test]
    fn second_point_commits_a_two_vertex_wall_polyline_with_xdata() {
        let mut cmd = WallCommand::new_with_library(None);
        assert!(matches!(
            cmd.on_point(DVec3::new(0.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));

        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => {
                match &entity {
                    EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
                    _ => panic!("expected a wall polyline"),
                }
                assert!(
                    wall_xdata(&entity).is_some(),
                    "committed wall segment should carry OPENCAD_AEC/WALL xdata"
                );
                assert_eq!(
                    crate::entities::names::ui_name(&entity),
                    "Wall",
                    "properties/status should label the host as Wall, not Polyline"
                );
            }
            _ => panic!("second point should commit a wall polyline"),
        }
        assert_eq!(cmd.vertices.len(), 1);
        assert_eq!(cmd.segments_committed, 1);
    }

    #[test]
    fn third_point_commits_a_second_two_vertex_wall() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected CommitEntity"),
        };
        match first {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
            _ => panic!("expected LwPolyline"),
        }
        match cmd.on_point(DVec3::new(5.0, 3.0, 0.0)) {
            CmdResult::CommitEntity(entity) => match entity {
                EntityType::LwPolyline(pl) => assert_eq!(pl.vertices.len(), 2),
                _ => panic!("expected a wall polyline"),
            },
            _ => panic!("a third point should commit the next 2-vertex wall"),
        }
        assert_eq!(cmd.segments_committed, 2);
        assert_eq!(cmd.vertices.len(), 1);
    }

    #[test]
    fn zero_length_second_point_is_rejected() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(1.0, 1.0, 0.0));
        assert!(matches!(
            cmd.on_point(DVec3::new(1.0, 1.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert_eq!(cmd.vertices.len(), 1);
        assert_eq!(cmd.segments_committed, 0);
    }

    #[test]
    fn wall_chain_auto_joins_l_corner() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let e1 = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("expected first segment"),
        };
        let h1 = scene.add_entity(e1);
        cmd.on_entities_committed(&mut scene, &[h1]);
        let e2 = match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("expected second segment"),
        };
        let h2 = scene.add_entity(e2);
        cmd.on_entities_committed(&mut scene, &[h2]);

        let w1 = wall_from_entity(scene.document.get_entity(h1).unwrap()).unwrap();
        let w2 = wall_from_entity(scene.document.get_entity(h2).unwrap()).unwrap();
        assert!(!w1.derived_handles.is_empty());
        assert!(!w2.derived_handles.is_empty());
        let a1 = get_wall_vertices(&scene, h1);
        let a2 = get_wall_vertices(&scene, h2);
        let corner = DVec3::new(5.0, 0.0, 0.0);
        assert!(a1.iter().any(|p| p.distance(corner) < 1e-4));
        assert!(a2.iter().any(|p| p.distance(corner) < 1e-4));
    }

    #[test]
    fn two_point_wall_regen_creates_contour_hatch_solid() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("expected segment"),
        };
        let handle = scene.add_entity(entity);
        cmd.on_entities_committed(&mut scene, &[handle]);
        let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        let mut saw_contour = false;
        let mut saw_hatch = false;
        let mut saw_solid = false;
        for h in wall.derived_handles {
            let entity = scene.document.get_entity(h).unwrap();
            let record = read_aec_record(entity).expect("display child");
            if let [XDataValue::String(kind), _, XDataValue::String(role)] = record.values.as_slice()
            {
                assert_eq!(kind, "WALL_REP");
                match role.as_str() {
                    WALL_REP_ROLE_CONTOUR => saw_contour = true,
                    WALL_REP_ROLE_HATCH => saw_hatch = true,
                    WALL_REP_ROLE_SOLID => saw_solid = true,
                    _ => {}
                }
            }
        }
        assert!(saw_contour && saw_hatch && saw_solid);
    }

    #[test]
    fn wall_commit_erases_live_preview_axis_and_contour() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let mut live_axis = LwPolyline::new();
        live_axis.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        live_axis.add_vertex(LwVertex::new(Vector2::new(3.0, 0.0)));
        let mut live_contour = LwPolyline::new();
        live_contour.is_closed = true;
        live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
        live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, -0.1)));
        live_contour.add_vertex(LwVertex::new(Vector2::new(3.0, 0.1)));
        live_contour.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
        let live_handles = vec![
            scene.add_entity(EntityType::LwPolyline(live_axis)),
            scene.add_entity(EntityType::LwPolyline(live_contour)),
        ];
        cmd.set_live_handles(live_handles.clone());
        let committed = match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("expected commit"),
        };
        let handle = scene.add_entity(committed);
        cmd.on_entities_committed(&mut scene, &[handle]);
        for h in live_handles {
            if h != handle {
                assert!(
                    scene.document.get_entity(h).is_none(),
                    "live preview {h:?} should be erased"
                );
            }
        }
        let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert!(!wall.derived_handles.is_empty());
    }

    #[test]
    fn arc_keyword_toggles_arc_mode_and_produces_a_bulge_segment() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let first = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected first segment"),
        };
        match first {
            EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
            _ => panic!("expected LwPolyline"),
        }

        assert!(matches!(cmd.on_text_input("A"), Some(CmdResult::NeedPoint)));
        assert!(cmd.arc_mode);
        let arc_entity = match cmd.on_point(DVec3::new(10.0, 5.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected arc segment"),
        };
        match arc_entity {
            EntityType::LwPolyline(pl) => {
                assert_eq!(pl.vertices.len(), 2);
                assert_ne!(pl.vertices[0].bulge, 0.0, "arc-mode segment should get a bulge");
            }
            _ => panic!("expected LwPolyline"),
        }

        assert!(matches!(cmd.on_text_input("L"), Some(CmdResult::NeedPoint)));
        assert!(!cmd.arc_mode);
        match cmd.on_point(DVec3::new(15.0, 5.0, 0.0)) {
            CmdResult::CommitEntity(entity) => match entity {
                EntityType::LwPolyline(pl) => assert_eq!(pl.vertices[0].bulge, 0.0),
                _ => panic!("expected LwPolyline"),
            },
            _ => panic!("expected straight segment"),
        }
    }

    #[test]
    fn first_segment_in_arc_mode_commits_on_second_click_with_bulge() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert!(matches!(cmd.on_text_input("ARC"), Some(CmdResult::NeedPoint)));
        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
                assert_ne!(pl.vertices[0].bulge, 0.0);
            }
            _ => panic!("expected committed first-segment arc"),
        }
    }

    #[test]
    fn commits_two_vertex_segments_without_picking_a_style() {
        let mut cmd = WallCommand::new();
        assert!(
            cmd.style_id.is_some(),
            "seed library should auto-select a wall style"
        );
        assert!(
            cmd.requires_style_selection(),
            "seed library should offer wall styles"
        );
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        match cmd.on_point(DVec3::new(4.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
                assert_eq!(pl.vertices.len(), 2);
            }
            _ => panic!("expected a 2-vertex commit"),
        }
        match cmd.on_point(DVec3::new(4.0, 3.0, 0.0)) {
            CmdResult::CommitEntity(EntityType::LwPolyline(pl)) => {
                assert_eq!(pl.vertices.len(), 2);
            }
            _ => panic!("expected a second 2-vertex commit"),
        }
        assert_eq!(cmd.segments_committed, 2);
        assert_eq!(cmd.vertices.len(), 1);
    }

    #[test]
    fn third_segment_keeps_first_corner_joined() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let h1 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("seg 1"),
        });
        cmd.on_entities_committed(&mut scene, &[h1]);
        let h2 = scene.add_entity(match cmd.on_point(DVec3::new(5.0, 4.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("seg 2"),
        });
        cmd.on_entities_committed(&mut scene, &[h2]);
        let h3 = scene.add_entity(match cmd.on_point(DVec3::new(0.0, 4.0, 0.0)) {
            CmdResult::CommitEntity(e) => e,
            _ => panic!("seg 3"),
        });
        cmd.on_entities_committed(&mut scene, &[h3]);

        let peers2 = engine::owner_index::peers_of(&scene.document, h2);
        assert!(
            peers2.contains(&h1) && peers2.contains(&h3),
            "middle wall should stay joined to both neighbors, got {peers2:?}"
        );
        refresh_wall_after_axis_edit(&mut scene, h2, None, None, None);
        let peers2_after = engine::owner_index::peers_of(&scene.document, h2);
        assert!(
            peers2_after.contains(&h1) && peers2_after.contains(&h3),
            "regen/edit must not drop either join, got {peers2_after:?}"
        );
    }

    #[test]
    fn undo_drops_the_last_vertex_and_removes_the_live_entity_below_two_points() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert!(matches!(cmd.on_text_input("U"), Some(CmdResult::NeedPoint)));
        assert!(cmd.vertices.is_empty());
    }

    #[test]
    fn enter_before_two_points_keeps_the_command_running() {
        // Enter/Escape are global "finalize" keys that can fire while the
        // user is still only editing the live height/style panel fields
        // before the axis has a second point — there is nothing to finalize
        // yet, so the command must stay alive instead of cancelling.
        let mut enter_cmd = WallCommand::new_with_library(None);
        assert!(matches!(enter_cmd.on_enter(), CmdResult::NeedPoint));

        let mut escape_cmd = WallCommand::new_with_library(None);
        assert!(matches!(escape_cmd.on_escape(), CmdResult::NeedPoint));

        let mut one_point_cmd = WallCommand::new_with_library(None);
        one_point_cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert!(matches!(one_point_cmd.on_enter(), CmdResult::NeedPoint));
    }

    #[test]
    fn enter_after_the_point_chain_finalizes_immediately_with_defaults() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected segment commit"),
        };
        let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
        assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
        assert!((wall.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn live_height_edit_before_enter_finalizes_with_the_edited_value() {
        use crate::command::LiveFieldValue;

        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected segment commit"),
        };
        let wall = wall_from_entity(&entity).expect("committed wall should carry WALL xdata");
        assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
        assert!((wall.height - 3.5).abs() < 1e-9);
    }

    /// Draw four wall segments through `WallCommand` exactly as the
    /// interactive host would (point chain, then height/thickness prompt),
    /// then commit each finalized entity into a real `Scene`. Regression
    /// check for `AEC_ROOM`'s closed-loop detection against interactively
    /// drawn walls (previously only exercised against demo geometry).
    #[test]
    fn aec_room_detects_a_closed_loop_from_interactively_drawn_walls() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for pair in corners.windows(2) {
            let mut cmd = WallCommand::new_with_library(None);
            let committed = match cmd.on_point(pair[0]) {
                CmdResult::NeedPoint => cmd.on_point(pair[1]),
                other => other,
            };
            let entity = match committed {
                CmdResult::CommitEntity(entity) => entity,
                _ => panic!("two points should commit a wall segment"),
            };
            let handle = scene.add_entity(entity);
            cmd.on_entities_committed(&mut scene, &[handle]);
            assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!(
            (area - 12.0).abs() < 1e-6,
            "expected the detected 4x3 wall loop to yield area 12.0, got {area}"
        );
    }

    /// `wall_from_entity` is the inverse of `wall_record` — the properties
    /// panel reads a `Wall` this way to populate wall fields for a WALL-tagged
    /// entity.
    #[test]
    fn wall_from_entity_reads_back_a_finalized_wall_record() {
        use crate::command::LiveFieldValue;

        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("expected second point to commit the wall"),
        };

        let wall = wall_from_entity(&entity).expect("finalized entity should read back as a Wall");
        assert!((wall.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);
        assert!((wall.height - 3.5).abs() < 1e-9);
        assert_eq!(wall.layers.len(), 1);
    }

    /// A plain (non-WALL-tagged) entity must not be misread as a wall — this
    /// is what keeps the properties-panel Wall section from appearing on
    /// regular polylines.
    #[test]
    fn wall_from_entity_returns_none_for_a_plain_polyline() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        assert!(wall_from_entity(&entity).is_none());
    }

    /// Height writeback via `write_wall_height` keeps the rest of the WALL
    /// record intact and still visible to the room segment collector.
    #[test]
    fn write_wall_height_updates_the_wall_xdata_in_place() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("two points should commit a wall segment"),
        };
        let handle = scene.add_entity(entity);

        assert!(write_wall_height(&mut scene, handle, 3.2));

        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("entity should still read back as a wall after the edit");
        assert!((updated.height - 3.2).abs() < 1e-9);
        assert!((updated.total_thickness() - DEFAULT_WALL_THICKNESS).abs() < 1e-9);

        // The AEC_ROOM segment collector still sees this wall after the edit.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 1);
    }

    /// `write_wall_hatch_override` (Step 6 Properties panel section) sets,
    /// updates, and clears the per-wall-instance hatch-angle override while
    /// leaving the rest of the WALL record intact.
    #[test]
    fn write_wall_hatch_override_sets_updates_and_clears_the_override() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("two points should commit a wall segment"),
        };
        let handle = scene.add_entity(entity);

        // No override set initially.
        let initial = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(initial.hatch_override, None);

        // Setting an override.
        let override_a = engine::display_component::ComponentStyleOverride {
            hatch_angle: Some(45.0),
            hatch_angle_relative: Some(false),
            ..Default::default()
        };
        assert!(write_wall_hatch_override(&mut scene, handle, Some(override_a.clone())));
        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(updated.hatch_override, Some(override_a));
        // Unrelated fields untouched.
        assert!((updated.height - DEFAULT_WALL_HEIGHT).abs() < 1e-9);

        // Updating the override (e.g. angle field edited again).
        let override_b = engine::display_component::ComponentStyleOverride {
            hatch_angle: Some(90.0),
            hatch_angle_relative: Some(true),
            ..Default::default()
        };
        assert!(write_wall_hatch_override(&mut scene, handle, Some(override_b.clone())));
        let updated2 = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(updated2.hatch_override, Some(override_b));

        // Clearing the override (checkbox unchecked).
        assert!(write_wall_hatch_override(&mut scene, handle, None));
        let cleared = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        assert_eq!(cleared.hatch_override, None);
    }

    #[test]
    fn apply_phase_filter_with_no_filter_shows_everything_unstyled() {
        let new_phase = apply_phase_filter(PlanPhase::New, None);
        assert!(new_phase.visible);
        assert_eq!(new_phase.extra_style, None);

        let demolition = apply_phase_filter(PlanPhase::Demolition, None);
        assert!(demolition.visible);
        assert_eq!(
            demolition.extra_style.unwrap().line_type.as_deref(),
            Some("DASHED")
        );

        let existing = apply_phase_filter(PlanPhase::Existing, None);
        assert!(existing.visible);
        assert_eq!(
            existing.extra_style.unwrap().line_color,
            Some(acadrust::types::Color::Rgb {
                r: 136,
                g: 136,
                b: 136
            })
        );
    }

    #[test]
    fn apply_phase_filter_hides_phases_missing_from_visible_phases() {
        let filter = PhaseFilter {
            visible_phases: vec![PlanPhase::New],
            demolition_style: None,
            existing_style: None,
        };
        assert!(apply_phase_filter(PlanPhase::New, Some(&filter)).visible);
        assert!(!apply_phase_filter(PlanPhase::Demolition, Some(&filter)).visible);
        assert!(!apply_phase_filter(PlanPhase::Existing, Some(&filter)).visible);
    }

    #[test]
    fn apply_phase_filter_applies_demolition_and_existing_overlays() {
        let filter = PhaseFilter {
            visible_phases: vec![PlanPhase::New, PlanPhase::Demolition, PlanPhase::Existing],
            demolition_style: Some(engine::display_component::ComponentStyleOverride {
                line_type: Some("Dashed".to_string()),
                ..Default::default()
            }),
            existing_style: Some(engine::display_component::ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }),
                ..Default::default()
            }),
        };
        let demolition = apply_phase_filter(PlanPhase::Demolition, Some(&filter));
        assert!(demolition.visible);
        assert_eq!(demolition.extra_style.unwrap().line_type, Some("Dashed".to_string()));

        let existing = apply_phase_filter(PlanPhase::Existing, Some(&filter));
        assert!(existing.visible);
        assert_eq!(existing.extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 136, g: 136, b: 136 }));

        // `New` walls never pick up an overlay, even if visible.
        let new_phase = apply_phase_filter(PlanPhase::New, Some(&filter));
        assert!(new_phase.visible);
        assert_eq!(new_phase.extra_style, None);
    }

    #[test]
    fn merge_phase_extra_only_overrides_contour2d() {
        let extra = engine::display_component::ComponentStyleOverride {
            line_type: Some("DASHED".to_string()),
            line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
            ..Default::default()
        };
        let mut rules = engine::display_component::ComponentRuleSet::default();
        rules.visibility.insert(
            engine::display_component::WallComponentSlot::Layers2D
                .key()
                .to_string(),
            true,
        );
        merge_phase_extra_into_rules(&mut rules, extra.clone());
        let contour = rules
            .style_for(engine::display_component::WallComponentSlot::Contour2D)
            .expect("phase extra on envelope");
        assert_eq!(contour.line_type.as_deref(), Some("DASHED"));
        assert_eq!(
            contour.line_color,
            Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
        );
        assert!(
            rules
                .style_for(engine::display_component::WallComponentSlot::Layers2D)
                .is_none()
        );
        assert!(
            rules
                .style_for(engine::display_component::WallComponentSlot::LayerHatch2D)
                .is_none()
        );
        assert!(
            rules
                .style_for(engine::display_component::WallComponentSlot::Solid3D)
                .is_none()
        );
    }

    #[test]
    fn wall_round_trip() {
        let layers = wls(&[
            ("Finish", 0.02, "Finish"),
            ("Brick", 0.10, "Structural"),
            ("Finish", 0.02, "Finish"),
        ]);
        let values = wall_record("style1", 3.0, 1, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("Should parse WALL");
        assert_eq!(wall.style_id, "style1");
        assert_eq!(wall.height, 3.0);
        assert_eq!(wall.storey_id, 1);
        assert_eq!(wall.layers.len(), 3);
        assert_eq!(wall.layers[1].material, "Brick");
        assert_eq!(wall.layers[1].thickness, 0.10);
        assert_eq!(wall.layers[1].function, "Structural");
        assert_eq!(wall.total_thickness(), 0.14);
        assert_eq!(wall.phase, PlanPhase::New);
    }

    #[test]
    fn wall_from_entity_round_trips_phase() {
        let layers = wls(&[("Brick", 0.2, "Structural")]);
        let values = wall_record(
            "style1",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center,
            PlanPhase::Demolition,
            None,
        );
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("Should parse WALL");
        assert_eq!(wall.phase, PlanPhase::Demolition);
    }

    #[test]
    fn wall_without_phase_tail_defaults_to_new() {
        // Simulates a record written before the `phase` field existed: the
        // trailing tag is simply absent, and parsing must fall back to
        // `PlanPhase::New` rather than failing.
        let layers = wls(&[("Brick", 0.2, "Structural")]);
        let mut values = wall_record(
            "style1",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center,
            PlanPhase::Demolition,
            None,
        );
        // Drop the trailing wall-hatch-override block (3 values) and the
        // phase tag itself, simulating a record written before either
        // field existed.
        for _ in 0..4 {
            values.pop();
        }
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("Should parse WALL");
        assert_eq!(wall.phase, PlanPhase::New);
    }

    #[test]
    fn write_wall_height_preserves_phase() {
        let mut scene = Scene::new();
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        let entity = match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("two points should commit a wall segment"),
        };
        let handle = scene.add_entity(entity);

        // Manually flip the phase to `Existing`, mimicking a prior
        // Properties-panel edit, then confirm a later height edit preserves it.
        let mut wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        wall.phase = PlanPhase::Existing;
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            &wall.style_id,
            wall.height,
            wall.storey_id,
            &wall.layers,
            &wall.derived_handles,
            wall.justification,
            wall.phase,
            wall.hatch_override.as_ref(),
        );
        write_aec_record(&mut scene.document, handle, record);

        assert!(write_wall_height(&mut scene, handle, 3.2));

        let updated = wall_from_entity(scene.document.get_entity(handle).unwrap())
            .expect("entity should still read back as a wall after the edit");
        assert_eq!(updated.phase, PlanPhase::Existing);
        assert!((updated.height - 3.2).abs() < 1e-9);
    }

    #[test]
    fn wall_thickness_and_height_reads_wall_record() {
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Mat", 0.15, "Func")];
        let mut rec = ExtendedDataRecord::new(AEC_APPID);
        rec.values = wall_record("style2", 3.2, 2, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(rec);

        let res = wall_thickness_and_height(&entity).expect("Should read WALL");
        assert_eq!(res, (0.15, 3.2, 2));
    }

    #[test]
    fn aec_room_detects_a_closed_loop_from_walls() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        for pair in corners.windows(2) {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
            pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("Brick", 0.2, "Structural")];
            let mut r = ExtendedDataRecord::new(AEC_APPID);
            r.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(r);
            scene.add_entity(entity);
        }

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);

        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!((area - 12.0).abs() < 1e-6);
    }

    #[test]
    fn wall_command_with_library_uses_ask_style_and_finalizes() {
        use crate::command::LiveFieldValue;
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: LayerValue::Fixed(0.25),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.0));
        cmd.apply_live_property(
            "wall_style",
            LiveFieldValue::Picker("style1".to_string()),
        );
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => {
                let pl = match &entity {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a wall polyline"),
                };
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
                assert_eq!(record.values[1], XDataValue::String("style1".to_string()));
                assert!(
                    matches!(record.values[2], XDataValue::Distance(h) if (h - 3.0).abs() < 1e-9)
                );
                assert_eq!(
                    record.values[5],
                    XDataValue::String("Brick Material".to_string())
                );
                assert!(
                    matches!(record.values[6], XDataValue::Distance(t) if (t - 0.25).abs() < 1e-9)
                );
                assert_eq!(
                    record.values[7],
                    XDataValue::String("Structural".to_string())
                );
            }
            _ => panic!("expected second click to commit a styled segment"),
        }
    }

    #[test]
    fn wall_command_live_properties_reports_style_and_height_while_drawing() {
        use crate::command::LiveFieldValue;

        let mut cmd = WallCommand::new_with_library(None);
        // Still in the Drawing phase (no points yet): live_properties should
        // be available with the default height and an empty style name.
        let live = cmd.live_properties().expect("Drawing phase should expose live properties");
        assert_eq!(live.title, "Wall");
        assert_eq!(live.fields.len(), 3);
        assert_eq!(live.fields[0].field_id, "wall_style");
        assert_eq!(live.fields[1].field_id, "wall_height");
        assert_eq!(live.fields[2].field_id, "wall_justification");
        assert!(matches!(&live.fields[1].value, LiveFieldValue::Number(h) if (*h - DEFAULT_WALL_HEIGHT).abs() < 1e-9));

        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));

        cmd.apply_live_property("wall_height", LiveFieldValue::Number(3.5));
        assert!((cmd.wall.height - 3.5).abs() < 1e-9);

        let live_after = cmd.live_properties().expect("still drawing");
        assert!(matches!(&live_after.fields[1].value, LiveFieldValue::Number(h) if (*h - 3.5).abs() < 1e-9));
    }

    #[test]
    fn wall_command_apply_live_property_updates_style_and_resolved_layers() {
        use crate::command::LiveFieldValue;
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: LayerValue::Fixed(0.25),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        cmd.on_point(DVec3::new(5.0, 0.0, 0.0));
        cmd.set_live_handles(vec![Handle::new(201)]);

        cmd.apply_live_property(
            "wall_style",
            LiveFieldValue::Picker("style1".to_string()),
        );

        assert_eq!(cmd.style_id.as_deref(), Some("style1"));
        let layers = cmd.resolved_layers.clone().expect("style pick should resolve layers");
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].material, "Brick Material");
        assert!((layers[0].thickness - 0.25).abs() < 1e-9);
        assert_eq!(layers[0].function, "Structural");

        let live = cmd.live_properties().expect("still drawing");
        assert!(matches!(&live.fields[0].value, LiveFieldValue::Picker(s) if s == "Standard Wall"));
    }

    #[test]
    fn wall_command_refuses_to_finish_without_a_style_when_styles_are_available() {
        use crate::modules::aec::engine::material::Material;
        use crate::modules::aec::engine::style::Style;
        use crate::modules::aec::engine::wall_style::{Layer, LayerFunction, WallStyle};

        let material = Material::new(
            "brick_id".to_string(),
            "Brick Material".to_string(),
            "ANSI31".to_string(),
            0xFF0000,
            "Continuous".to_string(),
        );
        let style = WallStyle {
            style: Style {
                id: "style1".to_string(),
                name: "Standard Wall".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "brick_id".to_string(),
                thickness: LayerValue::Fixed(0.25),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        let lib = StyleLibrary {
            materials: vec![material],
            wall_styles: vec![style],
        };

        let mut cmd = WallCommand::new_with_library(Some(lib));
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(_) => {}
            _ => panic!("segment should commit even without a style"),
        }
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn wall_command_with_no_library_falls_back_to_v1_record() {
        let mut cmd = WallCommand::new_with_library(None);
        cmd.on_point(DVec3::new(0.0, 0.0, 0.0));
        match cmd.on_point(DVec3::new(5.0, 0.0, 0.0)) {
            CmdResult::CommitEntity(entity) => {
                let pl = match &entity {
                    EntityType::LwPolyline(pl) => pl,
                    _ => panic!("expected a wall polyline"),
                };
                let record = pl.common.extended_data.get_record(AEC_APPID).unwrap();
                assert_eq!(record.values[0], XDataValue::String("WALL".to_string()));
            }
            _ => panic!("Expected second point to commit wall with V1 record"),
        }
    }

    #[test]
    fn material_command_collects_fields_and_dispatches_add_command() {
        let mut cmd = MaterialCommand::new();
        assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));

        assert!(matches!(
            cmd.on_text_input("Sichtbeton"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-hatch"));

        assert!(matches!(
            cmd.on_text_input(""), // blank -> default hatch
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-color"));

        assert!(matches!(
            cmd.on_text_input("A0A0A0"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-linetype"));

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_MATERIAL_ADD Sichtbeton|ANSI31|A0A0A0|Continuous"
                );
            }
            _ => panic!("expected the final field to dispatch AEC_MATERIAL_ADD"),
        }
    }

    #[test]
    fn material_command_requires_a_non_empty_name() {
        let mut cmd = MaterialCommand::new();
        assert!(matches!(cmd.on_text_input(""), Some(CmdResult::NeedPoint)));
        // Still on the name step.
        assert_eq!(cmd.prompt(), crate::tr!("aec", "material-prompt-name"));
    }

    #[test]
    fn style_command_collects_two_layers_and_dispatches_add_command() {
        let mut cmd = StyleCommand::new();
        assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-name"));

        assert!(matches!(
            cmd.on_text_input("Testwand"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(cmd.prompt(), crate::tr!("aec", "style-prompt-parent"));

        assert!(matches!(
            cmd.on_text_input(""), // no parent
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            cmd.prompt(),
            crate::tr!("aec", "style-prompt-layer-material", n = 1i32)
        );

        // Layer 1
        assert!(matches!(
            cmd.on_text_input("Putz"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.015"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Finish"),
            Some(CmdResult::NeedPoint)
        ));

        // Layer 2
        assert!(matches!(
            cmd.on_text_input("Mauerwerk"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("0.24"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            cmd.on_text_input("Structural"),
            Some(CmdResult::NeedPoint)
        ));

        // Blank material name ends the layer loop and dispatches.
        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(
                    dispatch,
                    "AEC_STYLE_ADD Testwand||Putz:0.015:Finish;Mauerwerk:0.24:Structural"
                );
            }
            _ => panic!("expected the final blank layer entry to dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn style_command_with_no_layers_dispatches_empty_layer_list_for_inheritance() {
        let mut cmd = StyleCommand::new();
        cmd.on_text_input("Kind Wand");
        cmd.on_text_input("Standard Wall"); // parent

        match cmd.on_text_input("") {
            Some(CmdResult::Dispatch(dispatch)) => {
                assert_eq!(dispatch, "AEC_STYLE_ADD Kind Wand|Standard Wall|");
            }
            _ => panic!("expected an empty layer list to still dispatch AEC_STYLE_ADD"),
        }
    }

    #[test]
    fn layer_function_round_trips_through_its_plain_string_form() {
        for f in [
            LayerFunction::Structural,
            LayerFunction::Insulation,
            LayerFunction::Finish,
        ] {
            let s = layer_function_to_str(&f);
            assert_eq!(parse_layer_function(&s), f);
        }
        assert_eq!(parse_layer_function(""), LayerFunction::Structural);
        assert_eq!(
            parse_layer_function("Custom"),
            LayerFunction::Other("Custom".to_string())
        );
    }

    #[test]
    fn wall_justification_conversion() {
        // Horizontal wall along X axis from 0 to 10.
        // Thickness = 0.2.
        // Points picked at Interior (Y=+0.1 if drawing left to right).
        // Centerline should be at Y=0.

        let mut cmd = WallCommand::new_with_library(None);
        cmd.justification = WallJustification::Interior;
        cmd.thickness = 0.2;

        cmd.on_point(DVec3::new(0.0, 0.1, 0.0));
        let entity = match cmd.on_point(DVec3::new(10.0, 0.1, 0.0)) {
            CmdResult::CommitEntity(entity) => entity,
            _ => panic!("Should commit entity"),
        };
        let EntityType::LwPolyline(pl) = entity else {
            panic!("Expected LwPolyline")
        };

        assert_eq!(pl.vertices.len(), 2);
        // Interior justification for a segment (0, 0.1) -> (10, 0.1)
        // normal is (0, 1).
        // offset in build_entity is -0.1.
        // Centerline = Picked + (0, 1) * -0.1 = (0, 0).
        assert!((pl.vertices[0].location.x - 0.0).abs() < 1e-9);
        assert!((pl.vertices[0].location.y - 0.0).abs() < 1e-9);
        assert!((pl.vertices[1].location.x - 10.0).abs() < 1e-9);
        assert!((pl.vertices[1].location.y - 0.0).abs() < 1e-9);
    }

    #[test]
    fn wall_justification_toggle_cycle() {
        let mut cmd = WallCommand::new();
        assert_eq!(cmd.justification, WallJustification::Center);

        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Exterior);
        cmd.set_ctrl(true); // Should not toggle again while held
        assert_eq!(cmd.justification, WallJustification::Exterior);

        cmd.set_ctrl(false);
        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Interior);

        cmd.set_ctrl(false);
        cmd.set_ctrl(true);
        assert_eq!(cmd.justification, WallJustification::Center);
    }

    #[test]
    fn slugify_normalizes_names_into_stable_ids() {
        assert_eq!(slugify("Wand Stahlbeton 20cm"), "wand_stahlbeton_20cm");
        assert_eq!(slugify("  spaced  out  "), "spaced_out");
        assert_eq!(slugify(""), "item");
    }

    #[test]
    fn unique_id_stays_readable_but_differs_across_calls() {
        // Two ids generated for the exact same name (e.g. the same wall
        // style name entered independently in two different projects) must
        // still differ, so libraries created independently never collide
        // when later mixed (see `unique_id` doc comment).
        let a = unique_id("style", "Mauerwerk 36.5");
        let b = unique_id("style", "Mauerwerk 36.5");
        assert_ne!(a, b);
        assert!(a.starts_with("style_mauerwerk_36_5_"));
        assert!(b.starts_with("style_mauerwerk_36_5_"));
    }

    #[test]
    fn wall_round_trip_with_derived_handles() {
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let derived = vec![Handle::new(10), Handle::new(11), Handle::new(12)];
        let values = wall_record("style1", 3.0, 0, &layers, &derived, WallJustification::Center, PlanPhase::New, None);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("Should parse WALL");
        assert_eq!(wall.derived_handles, derived);
    }

    #[test]
    fn wall_without_derived_handles_tail_still_parses() {
        // Simulate an old record written before `derived_handles` existed:
        // build it with an empty list and confirm it reads back empty, not
        // an error, keeping legacy records readable.
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("Should parse WALL");
        assert!(wall.derived_handles.is_empty());
    }

    /// Build a two-layer `WALL` axis polyline in `scene` and return its
    /// handle.
    fn add_multi_layer_wall(scene: &mut Scene) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = wls(&[
            ("Concrete", 0.2, "Structural"),
            ("Insulation", 0.05, "Insulation"),
        ]);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    #[test]
    fn demolition_regen_sets_dashed_envelope_linetype() {
        let mut scene = Scene::new();
        let handle = add_multi_layer_wall(&mut scene);
        assert!(write_wall_phase(&mut scene, handle, PlanPhase::Demolition));
        let extra = apply_phase_filter(PlanPhase::Demolition, None)
            .extra_style
            .expect("demolition extra");
        let mut rules = engine::display_component::ComponentRuleSet::default();
        merge_phase_extra_into_rules(&mut rules, extra);
        regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            handle,
            Some(&rules),
            None,
            None,
        )
        .expect("regen");
        let wall = wall_from_entity(scene.document.get_entity(handle).unwrap()).unwrap();
        let dashed = wall
            .derived_handles
            .iter()
            .filter_map(|h| scene.document.get_entity(*h))
            .filter(|e| matches!(e, EntityType::LwPolyline(_)))
            .filter(|e| e.common().linetype.eq_ignore_ascii_case("DASHED"))
            .count();
        assert!(
            dashed >= 1,
            "demolition envelope contour must use DASHED linetype"
        );
    }

    /// Mirrors the core write-back logic of the `AecStylePickerConfirm`
    /// handler for `StylePickerTarget::WallPropertiesStyle` (see
    /// `src/app/update/mod.rs`): resolve `effective_layers()` for the newly
    /// chosen style from the currently loaded library, then write the new
    /// `style_id` + resolved layer snapshot back into the wall's `WALL`
    /// XDATA and regenerate its representation.
    #[test]
    fn changing_wall_properties_style_updates_style_id_and_layer_snapshot() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);

        let mut wall_styles: HashMap<String, WallStyle> = HashMap::new();
        wall_styles.insert(
            "style1".to_string(),
            WallStyle {
                style: Style {
                    id: "style1".to_string(),
                    name: "Style One".to_string(),
                    object_kind: "Wall".to_string(),
                    parent_style_id: None,
                },
                layers: vec![Layer {
                    material_id: "Concrete".to_string(),
                    thickness: LayerValue::Fixed(0.2),
                    function: LayerFunction::Structural,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                }],
            display_profiles: std::collections::HashMap::new(),
            },
        );
        wall_styles.insert(
            "style2".to_string(),
            WallStyle {
                style: Style {
                    id: "style2".to_string(),
                    name: "Style Two".to_string(),
                    object_kind: "Wall".to_string(),
                    parent_style_id: None,
                },
                layers: vec![
                    Layer {
                        material_id: "Brick".to_string(),
                        thickness: LayerValue::Fixed(0.1),
                        function: LayerFunction::Finish,
                        axis_offset: LayerValue::Fixed(0.0),
                        bottom_offset: 0.0,
                        top_offset: 0.0,
                        layer_override: None,
                        hatch_override: None,
                        role_tag: None,
                    layer_id: uuid::Uuid::new_v4(),
                    },
                    Layer {
                        material_id: "Insulation".to_string(),
                        thickness: LayerValue::Fixed(0.06),
                        function: LayerFunction::Insulation,
                        axis_offset: LayerValue::Fixed(0.0),
                        bottom_offset: 0.0,
                        top_offset: 0.0,
                        layer_override: None,
                        hatch_override: None,
                        role_tag: None,
                    layer_id: uuid::Uuid::new_v4(),
                    },
                ],
            display_profiles: std::collections::HashMap::new(),
            },
        );

        let new_style_id = "style2".to_string();
        let bb = super::engine::wall_style::base_width_from_layers(
            &super::engine::wall_style::effective_layers(&wall_styles, &new_style_id)
                .expect("style2 should resolve"),
        );
        let layers = effective_layers_for_wall_bb(&wall_styles, &new_style_id, bb)
            .expect("style2 should resolve");
        let wall_layers: Vec<WallLayer> = layers
            .into_iter()
            .map(|l| WallLayer {
                material: l.material_id.clone(),
                thickness: l.thickness,
                function: match &l.function {
                    LayerFunction::Structural => "Structural".to_string(),
                    LayerFunction::Insulation => "Insulation".to_string(),
                    LayerFunction::Finish => "Finish".to_string(),
                    LayerFunction::Other(s) => s.clone(),
                },
                axis_offset: l.axis_offset,
                bottom_offset: l.bottom_offset,
                top_offset: l.top_offset,
                layer_override: l.layer_override.clone(),
                hatch_override: l.hatch_override.clone(),
            layer_id: uuid::Uuid::new_v4(),
            })
            .collect();

        let mut wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should parse as WALL");
        wall.style_id = new_style_id.clone();
        wall.layers = wall_layers.clone();

        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            &wall.style_id,
            wall.height,
            wall.storey_id,
            &wall.layers,
            &wall.derived_handles,
            wall.justification, wall.phase, wall.hatch_override.as_ref());

        let entity = scene.document.get_entity_mut(wall_handle).unwrap();
        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != AEC_APPID)
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(record);

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed after style change");

        let updated = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still parse as WALL");
        assert_eq!(updated.style_id, "style2");
        assert_eq!(updated.layers.len(), 2);
        assert_eq!(updated.layers[0].material, "Brick");
        assert_eq!(updated.layers[1].material, "Insulation");
    }

    #[test]
    fn resolve_wall_package_returns_axis_for_a_derived_entity() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        assert!(!derived.is_empty());

        for handle in derived {
            assert_eq!(resolve_wall_package(&scene, handle), wall_handle);
        }
    }

    #[test]
    fn resolve_wall_package_uses_derived_handles_without_child_xdata() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("WALL")
            .derived_handles;
        assert!(!derived.is_empty());
        let child = derived[0];
        if let Some(entity) = scene.document.get_entity_mut(child) {
            entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
        }
        assert_eq!(resolve_wall_package(&scene, child), wall_handle);
        let pkg = wall_package_handles(&scene, wall_handle);
        assert!(pkg.contains(&child));
        assert!(pkg.contains(&wall_handle));
    }

    #[test]
    fn resolve_wall_package_reads_wall_rep_when_not_first_aec_record() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");
        let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("WALL")
            .derived_handles[0];
        if let Some(entity) = scene.document.get_entity_mut(child) {
            let mut dummy = ExtendedDataRecord::new(AEC_APPID);
            dummy.add_value(XDataValue::String("OTHER".into()));
            let mut records = vec![dummy];
            records.extend(entity.common().extended_data.records().iter().cloned());
            entity.common_mut().extended_data.clear();
            for r in records {
                entity.common_mut().extended_data.add_record(r);
            }
        }
        assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    }

    #[test]
    fn resolve_wall_package_accepts_integer_axis_handle() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");
        let child = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("WALL")
            .derived_handles[0];
        if let Some(entity) = scene.document.get_entity_mut(child) {
            entity.common_mut().extended_data.clear();
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.add_value(XDataValue::String("WALL_REP".into()));
            record.add_value(XDataValue::Integer32(wall_handle.value() as i32));
            record.add_value(XDataValue::String(WALL_REP_ROLE_CONTOUR.into()));
            entity.common_mut().extended_data.add_record(record);
        }
        assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    }

    #[test]
    fn resolve_wall_package_matches_untagged_layer_polyline_by_geometry() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("WALL")
            .derived_handles;
        let child = derived
            .iter()
            .copied()
            .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(pl)) if pl.vertices.len() >= 3))
            .expect("layer contour");
        if let Some(entity) = scene.document.get_entity_mut(child) {
            entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
        }
        let Some(entity) = scene.document.get_entity(wall_handle) else {
            panic!("wall");
        };
        let mut wall = wall_from_entity(entity).expect("WALL");
        wall.derived_handles.clear();
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        for v in wall_record(
            &wall.style_id,
            wall.height,
            wall.storey_id,
            &wall.layers,
            &wall.derived_handles,
            wall.justification,
            wall.phase,
            wall.hatch_override.as_ref(),
        ) {
            record.add_value(v);
        }
        write_aec_record(&mut scene.document, wall_handle, record);
        assert_eq!(resolve_wall_package(&scene, child), wall_handle);
    }

    #[test]
    fn resolve_wall_package_matches_untagged_solid_by_geometry() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("WALL")
            .derived_handles;
        let child = derived
            .iter()
            .copied()
            .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::Solid3D(_))))
            .expect("layer solid");
        if let Some(entity) = scene.document.get_entity_mut(child) {
            entity.common_mut().extended_data = acadrust::xdata::ExtendedData::default();
        }
        let Some(entity) = scene.document.get_entity(wall_handle) else {
            panic!("wall");
        };
        let mut wall = wall_from_entity(entity).expect("WALL");
        wall.derived_handles.clear();
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        for v in wall_record(
            &wall.style_id,
            wall.height,
            wall.storey_id,
            &wall.layers,
            &wall.derived_handles,
            wall.justification,
            wall.phase,
            wall.hatch_override.as_ref(),
        ) {
            record.add_value(v);
        }
        write_aec_record(&mut scene.document, wall_handle, record);
        assert_eq!(resolve_wall_package(&scene, child), wall_handle);
        let expanded = expand_handles_for_wall_packages(&scene, &[child]);
        assert!(expanded.contains(&wall_handle));
    }

    #[test]
    fn regen_tags_display_children_with_wall_rep_roles() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        let mut saw_contour = false;
        let mut saw_hatch = false;
        let mut saw_solid = false;
        for handle in derived {
            let entity = scene.document.get_entity(handle).unwrap();
            let record = read_aec_record(entity).expect("display child must carry WALL_REP");
            match record.values.as_slice() {
                [XDataValue::String(kind), XDataValue::Handle(axis), XDataValue::String(role)] => {
                    assert_eq!(kind, "WALL_REP");
                    assert_eq!(*axis, wall_handle);
                    match role.as_str() {
                        WALL_REP_ROLE_CONTOUR => saw_contour = true,
                        WALL_REP_ROLE_HATCH => saw_hatch = true,
                        WALL_REP_ROLE_SOLID => saw_solid = true,
                        other => panic!("unexpected display role {other}"),
                    }
                }
                other => panic!("unexpected display XDATA {other:?}"),
            }
        }
        assert!(saw_contour && saw_hatch && saw_solid);
    }

    #[test]
    fn wall_hatch_scale_survives_document_clone_roundtrip() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        let hatch_handles: Vec<_> = derived
            .into_iter()
            .filter(|&handle| {
                matches!(
                    scene.document.get_entity(handle),
                    Some(EntityType::Hatch(_))
                )
            })
            .collect();
        assert!(!hatch_handles.is_empty());

        let before: Vec<(f32, f32)> = hatch_handles
            .iter()
            .map(|handle| {
                let model = scene.hatches.get(handle).expect("hatch model");
                (model.scale, effective_hatch_spacing(model))
            })
            .collect();

        let mut reloaded = Scene::new();
        reloaded.document = scene.document.clone();
        reloaded.populate_hatches_from_document();

        for (handle, (scale, spacing)) in hatch_handles.iter().zip(before) {
            let model = reloaded.hatches.get(handle).expect("reloaded hatch model");
            assert!(
                (model.scale - scale).abs() < 1e-5,
                "HatchModel.scale changed on reload: {} -> {}",
                scale,
                model.scale
            );
            assert!(
                (effective_hatch_spacing(model) - spacing).abs() < 1e-4,
                "effective hatch spacing changed on reload"
            );
        }
    }

    fn effective_hatch_spacing(model: &crate::scene::model::hatch_model::HatchModel) -> f32 {
        match &model.pattern {
            crate::scene::model::hatch_model::HatchPattern::Pattern(fams) => {
                fams.first().map(|f| f.dy.abs() * model.scale.max(1e-6)).unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }

    #[test]
    fn regenerating_many_walls_completes_quickly() {
        // Rough performance smoke test (not a micro-benchmark): regenerating
        // a batch of walls — mixing straight and curved axes, multiple
        // layers, and openings — must not show a gross performance
        // regression (e.g. an accidental O(n^2) added by a future change).
        // Generous wall-clock bound so this stays robust on slow/loaded CI
        // hardware while still catching an order-of-magnitude regression.
        use std::time::Instant;

        let mut scene = Scene::new();
        let mut handles = Vec::new();
        const WALL_COUNT: usize = 200;
        for i in 0..WALL_COUNT {
            let x0 = i as f64 * 6.0;
            let mut pl = LwPolyline::new();
            if i % 3 == 0 {
                // Every third wall is curved.
                let mut v0 = LwVertex::new(Vector2::new(x0, 0.0));
                v0.bulge = 0.3;
                pl.add_vertex(v0);
            } else {
                pl.add_vertex(LwVertex::new(Vector2::new(x0, 0.0)));
            }
            pl.add_vertex(LwVertex::new(Vector2::new(x0 + 5.0, 0.0)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values =
                wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            handles.push(scene.add_entity(entity));
        }

        let started = Instant::now();
        for h in &handles {
            regenerate_wall_representation(&mut scene, *h, None)
                .expect("regeneration should succeed for every generated wall");
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_secs() < 10,
            "regenerating {WALL_COUNT} walls took {elapsed:?}, expected well under 10s"
        );
    }

    #[test]
    fn tessellate_ring_with_bulges_is_a_noop_for_straight_rings() {
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)];
        let bulges = vec![0.0; 4];
        assert_eq!(tessellate_ring_with_bulges(&ring, &bulges), ring);
        // Absent bulges (shorter slice / empty) also stay a no-op.
        assert_eq!(tessellate_ring_with_bulges(&ring, &[]), ring);
    }

    #[test]
    fn tessellate_ring_with_bulges_densifies_arc_edges() {
        // A ring whose first edge is a semicircular arc (bulge = 1.0) from
        // (-1,0) to (1,0); the rest are straight closing edges.
        let ring = vec![(-1.0, 0.0), (1.0, 0.0), (1.0, -2.0), (-1.0, -2.0)];
        let bulges = vec![1.0, 0.0, 0.0, 0.0];
        let dense = tessellate_ring_with_bulges(&ring, &bulges);
        assert!(
            dense.len() > ring.len(),
            "an arc edge must be sampled into more than its two endpoints"
        );
        // Straight edges keep exactly their start vertex (no extra samples);
        // only the arc edge grows.
        assert_eq!(dense.len(), WALL_HATCH_ARC_SEGMENTS + 3);
        // The arc bulges outward from the chord: some sampled point should
        // be well off the y=0 chord line (the semicircle's midpoint sits a
        // full radius away, at y = ±1 depending on winding/bulge sign).
        assert!(dense.iter().any(|&(_, y)| y.abs() > 0.5));
    }

    #[test]
    fn regenerate_wall_representation_tessellates_hatch_boundary_for_curved_wall() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        // Semicircular arc axis (bulge = 1.0) so the layer footprint has a
        // curved edge.
        let mut v0 = LwVertex::new(Vector2::new(-2.0, 0.0));
        v0.bulge = 1.0;
        pl.add_vertex(v0);
        pl.add_vertex(LwVertex::new(Vector2::new(2.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a curved single-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;

        let mut found_dense_hatch = false;
        for h in derived {
            if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
                // A curved single-layer footprint has 4 raw vertices (2 per
                // side); the tessellated boundary must be denser.
                if hatch.paths.iter().any(|p| {
                    p.edges.iter().any(|e| {
                        matches!(e, acadrust::entities::hatch::BoundaryEdge::Polyline(pl) if pl.vertices.len() > 4)
                    })
                }) {
                    found_dense_hatch = true;
                }
            }
        }
        assert!(
            found_dense_hatch,
            "curved wall's hatch boundary should be tessellated into more than the raw 4 corner vertices"
        );
    }

    #[test]
    fn regen_uses_layer_hatch_override_for_hatch_pattern() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut overridden = wl("Concrete", 0.2, "Structural");
        overridden.hatch_override = Some("NET".to_string());
        let default_layer = wl("Insulation", 0.05, "Insulation");
        let layers = vec![overridden, default_layer];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;

        let mut saw_override_pattern = false;
        let mut saw_default_pattern = false;
        for h in derived {
            if let EntityType::Hatch(hatch) = scene.document.get_entity(h).unwrap() {
                if hatch.pattern.name == "NET" {
                    saw_override_pattern = true;
                } else if hatch.pattern.name == "ANSI31" {
                    saw_default_pattern = true;
                }
            }
        }
        assert!(
            saw_override_pattern,
            "the overridden layer's hatch should use the layer's hatch_override pattern"
        );
        assert!(
            saw_default_pattern,
            "the non-overridden layer should keep falling back to the material/default pattern"
        );
    }

    #[test]
    fn wall_from_entity_round_trips_layer_hatch_override() {
        let mut layer = wl("Concrete", 0.2, "Structural");
        layer.hatch_override = Some("NET".to_string());
        let layers = vec![layer, wl("Insulation", 0.05, "Insulation")];
        let values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        let pl = LwPolyline::new();
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let wall = wall_from_entity(&entity).expect("should parse WALL");
        assert_eq!(wall.layers[0].hatch_override.as_deref(), Some("NET"));
        assert_eq!(wall.layers[1].hatch_override, None);
    }

    #[test]
    fn layer_filter_for_contour_and_solid_can_differ_within_one_profile() {
        use engine::display_component::{ComponentRuleSet, LayerSelection, WallComponentSlot};
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let layer_ref = join::LayerRef { material_id: "Concrete".to_string(), role_tag: None, index: 0, layer_id: None };
        let mut rules = ComponentRuleSet::default();
        rules.layer_filter.insert(
            WallComponentSlot::Contour2D.key().to_string(),
            LayerSelection::Explicit(vec![layer_ref.clone()]),
        );
        // `Solid3D` left unset -> defaults to `All` (non-regression).
        let handles = regenerate_wall_representation_with_rules_and_substitutions(
            &mut scene,
            wall_handle,
            Some(&rules),
            None,
            None,
        )
        .expect("regeneration should succeed");
        assert!(!handles.is_empty());
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Contour2D), &LayerSelection::Explicit(vec![layer_ref]));
        assert_eq!(rules.layer_filter_for(WallComponentSlot::Solid3D), &LayerSelection::All);

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        let contour_count = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::LwPolyline(_))))
            .count();
        let solid_count = derived
            .iter()
            .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
            .count();
        assert_eq!(contour_count, 1, "Contour2D must include only the explicit layer");
        assert_eq!(solid_count, 2, "Solid3D must keep both layers when its filter defaults to All");
    }

    #[test]
    fn hatch_override_chain_wall_takes_precedence_over_material() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        let wall_hatch_override = engine::display_component::ComponentStyleOverride {
            hatch_angle: Some(45.0),
            hatch_angle_relative: Some(false),
            ..Default::default()
        };
        record.values = wall_record(
            "style1",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center,
            PlanPhase::New,
            Some(&wall_hatch_override),
        );
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should parse WALL");
        assert_eq!(wall.hatch_override, Some(wall_hatch_override));

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a single-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        let mut hatch_count = 0;
        for handle in derived {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(handle) {
                hatch_count += 1;
                assert!(
                    (hatch.pattern_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-5,
                    "wall hatch override should render at 45 degrees, got {} radians",
                    hatch.pattern_angle
                );
            }
        }
        assert_eq!(hatch_count, 1, "the single-layer wall should produce one hatch");
    }

    #[test]
    fn apply_phase_filter_hides_and_overlays_as_expected_end_to_end() {
        use engine::plan_view::{PhaseFilter, PlanPhase as Phase};
        let filter = PhaseFilter {
            visible_phases: vec![Phase::New, Phase::Demolition],
            demolition_style: Some(engine::display_component::ComponentStyleOverride {
                line_color: Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 }),
                ..Default::default()
            }),
            existing_style: None,
        };
        assert!(apply_phase_filter(Phase::New, Some(&filter)).visible);
        assert!(apply_phase_filter(Phase::Demolition, Some(&filter)).visible);
        assert!(!apply_phase_filter(Phase::Existing, Some(&filter)).visible);
        assert_eq!(
            apply_phase_filter(Phase::Demolition, Some(&filter)).extra_style.unwrap().line_color, Some(acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 })
        );
    }

    #[test]
    fn resolve_wall_package_returns_itself_for_a_non_wall_entity() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        assert_eq!(resolve_wall_package(&scene, handle), handle);
    }

    #[test]
    fn expand_handles_for_wall_packages_includes_owner_and_children() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        assert!(!derived.is_empty());

        let from_child = expand_handles_for_wall_packages(&scene, &[derived[0]]);
        let from_owner = expand_handles_for_wall_packages(&scene, &[wall_handle]);
        assert!(from_child.contains(&wall_handle));
        for handle in &derived {
            assert!(from_child.contains(handle));
            assert!(from_owner.contains(handle));
        }
        assert_eq!(from_child.len(), from_owner.len());
        assert!(!is_wall_derived_non_axis(&scene, wall_handle));
        for handle in &derived {
            assert!(is_wall_derived_non_axis(&scene, *handle));
        }
    }

    #[test]
    fn expand_with_wall_derived_handles_resolves_child_to_full_package() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        assert!(!derived.is_empty());

        let mut handles = vec![derived[0]];
        expand_with_wall_derived_handles(&scene, &mut handles);
        assert!(handles.contains(&wall_handle));
        for handle in &derived {
            assert!(handles.contains(handle));
        }
    }

    #[test]
    fn write_wall_height_from_derived_child_updates_owner() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");
        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles[0];
        assert!(write_wall_height(&mut scene, derived, 4.2));
        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("owner should still parse as WALL");
        assert!((wall.height - 4.2).abs() < 1e-9);
    }

    #[test]
    fn live_wall_properties_include_style_height_and_justification() {
        use crate::command::LiveFieldValue;
        let cmd = WallCommand::new_with_library(None);
        let props = cmd.live_properties().expect("drawing phase exposes live props");
        let ids: Vec<_> = props.fields.iter().map(|f| f.field_id).collect();
        assert!(ids.contains(&"wall_style"));
        assert!(ids.contains(&"wall_height"));
        assert!(ids.contains(&"wall_justification"));
        assert!(matches!(
            props.fields.iter().find(|f| f.field_id == "wall_justification"),
            Some(f) if matches!(f.value, LiveFieldValue::Choice { .. })
        ));
    }

    #[test]
    fn resolve_wall_package_falls_back_to_clicked_when_axis_is_missing() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        let handle = scene.add_entity(EntityType::LwPolyline(pl));

        // Tag it as derived from a handle that doesn't exist in the document.
        let stale_axis = Handle::new(999_999);
        write_wall_derived_tag(&mut scene, handle, stale_axis);

        assert_eq!(resolve_wall_package(&scene, handle), handle);
    }

    /// Bug 2: only the wall AXIS should ever be a snap candidate — never its
    /// derived shell/hatch/solid contours.
    #[test]
    fn wall_axis_snap_wires_excludes_derived_and_includes_axis() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL");
        assert!(!wall.derived_handles.is_empty());

        // Sanity: the axis lives on the invisible axis layer; its derived
        // contour does not.
        assert_eq!(
            scene.document.get_entity(wall_handle).unwrap().common().layer,
            AEC_WALL_AXIS_LAYER
        );
        let derived_handle = wall.derived_handles[0];
        assert_ne!(
            scene
                .document
                .get_entity(derived_handle)
                .unwrap()
                .common()
                .layer,
            AEC_WALL_AXIS_LAYER
        );

        // Simulate the generic (render-shared) wire set: since the axis
        // layer is off, only the derived entity's wire is present there —
        // the axis itself is entirely absent.
        let derived_entity = scene.document.get_entity(derived_handle).unwrap().clone();
        let raw_wires = std::sync::Arc::new(scene.wires_for_entities(&[derived_entity]));
        assert!(raw_wires
            .iter()
            .any(|w| w.name == derived_handle.value().to_string()));

        let filtered = wall_axis_snap_wires(&scene, raw_wires);

        assert!(
            !filtered
                .iter()
                .any(|w| w.name == derived_handle.value().to_string()),
            "wall-derived contour/hatch/solid wires must be excluded from snap candidates"
        );
        assert!(
            filtered
                .iter()
                .any(|w| w.name == wall_handle.value().to_string()),
            "the wall axis must be included as a snap candidate even though its layer is off"
        );
    }

    /// Bug 4: after two walls are joined, each wall's contour must extend
    /// past the shared corner into the other wall's footprint — verifying
    /// the pragmatic corner-overlap fix in `join_two_walls_in_document` /
    /// `regenerate_wall_representation_with_corner`.
    #[test]
    fn join_two_walls_extends_contours_into_shared_corner() {
        let mut scene = Scene::new();
        // Wall A: (0,0) -> (5,0), two layers, total thickness 0.25.
        let wall_a = add_multi_layer_wall(&mut scene);

        // Wall B: (6,1) -> (6,10), one layer, thickness 0.3. Meets wall A in
        // an L-junction at (6,0) — mirrors `join::tests::test_l_join`.
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None)
            .expect("wall A regeneration should succeed");
        regenerate_wall_representation(&mut scene, wall_b, None)
            .expect("wall B regeneration should succeed");

        let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("the two axes should join as an L-corner");
        assert_eq!(kind, JoinKind::L);

        // The trimmed axes must still meet exactly at the corner — the
        // corner-extension hint must not leak into the persisted axis.
        let axis_a = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
        assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

        // Wall A's contour (thickness 0.25) should reach past x=6 by (at
        // least close to) wall B's half thickness (0.15), overlapping into
        // wall B's own footprint instead of stopping flush at the corner.
        let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
            .expect("wall A should still read back as WALL");
        let max_x = wall_a_v2
            .derived_handles
            .iter()
            .filter_map(|h| scene.document.get_entity(*h))
            .filter_map(|e| match e {
                EntityType::LwPolyline(pl) => Some(pl),
                _ => None,
            })
            .flat_map(|pl| pl.vertices.iter().map(|v| v.location.x))
            .fold(f64::MIN, f64::max);
        assert!(
            max_x > 6.0 + 1e-6,
            "wall A's contour should extend past the corner into wall B's footprint, got max_x={max_x}"
        );
    }

    /// Step 2: a `NoExtend` `layer_pairs` override on wall A's structural
    /// layer must stop that specific layer from being extended into the L
    /// corner, while the other (insulation) layer still auto-miters exactly
    /// as in `join_two_walls_extends_contours_into_shared_corner`.
    #[test]
    fn join_junction_override_no_extend_keeps_one_layer_un_joined() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None)
            .expect("wall A regeneration should succeed");
        regenerate_wall_representation(&mut scene, wall_b, None)
            .expect("wall B regeneration should succeed");

        // Wall A's axis end that will join is its last vertex (end_index 1).
        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
        assert_eq!(
            read_junction_override(&scene, wall_a, 1),
            Some(override_data)
        );

        let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("the two axes should join as an L-corner");
        assert_eq!(kind, JoinKind::L);

        let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
            .expect("wall A should still read back as WALL");
        let contour_max_x: Vec<f64> = wall_a_v2
            .derived_handles
            .iter()
            .filter_map(|h| scene.document.get_entity(*h))
            .filter_map(|e| match e {
                EntityType::LwPolyline(pl) => Some(pl),
                _ => None,
            })
            .map(|pl| {
                pl.vertices
                    .iter()
                    .map(|v| v.location.x)
                    .fold(f64::MIN, f64::max)
            })
            .collect();
        assert!(
            !contour_max_x.is_empty(),
            "wall A should still have contour polylines"
        );
        // With the Concrete layer forced NoExtend, no contour piece should
        // reach past wall B's half-thickness the way the fully-automatic
        // regression case does — at most one derived contour (Insulation)
        // may still extend into the corner.
        let extending = contour_max_x.iter().filter(|&&x| x > 6.0 + 1e-6).count();
        assert!(
            extending <= 1,
            "the NoExtend Concrete layer must not extend past the corner, got max_x values {contour_max_x:?}"
        );
    }

    /// Step 4a: a `layer_style_override` set on one of two same-material
    /// layers (disambiguated by `layer_id`) must stay attached to the
    /// originally intended layer even after a third layer is inserted
    /// before it, shifting its `index`. Matching purely by `index` (or by
    /// `material_id` alone, since both layers share it) would either lose
    /// the override or misapply it to the wrong/both layers.
    #[test]
    fn layer_style_override_stays_attached_to_intended_layer_after_insert() {
        let target_id = uuid::Uuid::new_v4();
        let other_id = uuid::Uuid::new_v4();
        let rules = engine::display_component::ComponentRuleSet {
            layer_style_override: vec![engine::display_component::LayerStyleOverride {
                layer: join::LayerRef {
                    material_id: "Plaster".to_string(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(target_id),
                },
                style: engine::display_component::ComponentStyleOverride {
                    line_type: Some("Dashed".to_string()),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };

        // Original stack: [Plaster(other_id) @0, Plaster(target_id) @1].
        let original_ref = join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 1,
            layer_id: Some(target_id),
        };
        let before = resolve_layer_style_override(Some(&rules), &[], &original_ref);
        assert_eq!(before.line_type, Some("Dashed".to_string()));

        // A third layer is inserted before it, shifting the target layer's
        // index to 2 — but its `layer_id` is unchanged.
        let shifted_ref = join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 2,
            layer_id: Some(target_id),
        };
        let after = resolve_layer_style_override(Some(&rules), &[], &shifted_ref);
        assert_eq!(
            after.line_type,
            Some("Dashed".to_string()),
            "override must stay attached to the layer by layer_id even though its index moved"
        );

        // The other same-material layer (a different layer_id) must not
        // pick up the override just because the material matches.
        let other_ref = join::LayerRef {
            material_id: "Plaster".to_string(),
            role_tag: None,
            index: 0,
            layer_id: Some(other_id),
        };
        let other_result = resolve_layer_style_override(Some(&rules), &[], &other_ref);
        assert_eq!(other_result.line_type, None);
    }

    /// Step 4b: a `JunctionOverride.layer_pairs` entry referencing a layer by
    /// `layer_id` must stay correctly matched (and therefore survive
    /// `validate_junction_override`'s pruning pass) after an uninvolved
    /// layer is deleted, even though that shifts the referenced layer's
    /// `index`.
    #[test]
    fn junction_override_layer_pair_survives_deletion_of_uninvolved_layer() {
        let concrete_id = uuid::Uuid::new_v4();

        // The override was captured while the wall still had an Insulation
        // layer at index 0 and Concrete at index 1.
        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 1,
                    layer_id: Some(concrete_id),
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };

        // The uninvolved Insulation layer was deleted, so Concrete is now
        // the only (index 0) layer, keeping the same `layer_id`.
        let self_layers = vec![join::LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
            layer_id: Some(concrete_id),
        }];
        let other_layers: Vec<join::LayerRef> = vec![];

        let (validated, removed) = validate_junction_override(&override_data, &self_layers, &other_layers);
        assert_eq!(removed, 0, "the layer_id match must keep the pair despite the index shift");
        assert_eq!(
            validated,
            Some(join::JunctionOverride {
                default_style: None,
                layer_pairs: vec![join::LayerPairOverride {
                    layer_a: join::LayerRef {
                        material_id: "Concrete".to_string(),
                        role_tag: None,
                        index: 1,
                        layer_id: Some(concrete_id),
                    },
                    layer_b: None,
                    style: join::JoinOverrideStyle::NoExtend,
                }],
            })
        );
    }

    /// Step 3: when wall A's material changes such that a stored
    /// `LayerPairOverride.layer_a` no longer matches any current layer, that
    /// pair must be pruned on the next regeneration while a still-valid
    /// `default_style` on the same override survives, and regeneration must
    /// still succeed (falling back to automatic resolution for that layer).
    #[test]
    fn stale_layer_pair_override_is_pruned_but_default_style_kept() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

        let override_data = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Miter),
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

        // Structural change: wall A's "Concrete" layer becomes "Brick" — the
        // stored override's `layer_a` no longer matches anything on wall A.
        write_wall_layers(
            &mut scene,
            wall_a,
            wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
        );

        let _ = take_pending_override_warnings(); // clear anything queued so far
        let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("regeneration must succeed via automatic fallback");
        assert_eq!(kind, JoinKind::L);

        let cleaned = read_junction_override(&scene, wall_a, 1)
            .expect("default_style should survive the cleanup");
        assert_eq!(cleaned.default_style, Some(join::JoinOverrideStyle::Miter));
        assert!(
            cleaned.layer_pairs.is_empty(),
            "the stale Concrete layer pair should have been pruned, got {:?}",
            cleaned.layer_pairs
        );
    }

    /// Step 3: a layer removed entirely from a wall's style invalidates any
    /// override referencing it — same cleanup, no crash, and the wall's
    /// automatic-resolution footprint is still produced.
    #[test]
    fn override_referencing_removed_layer_is_cleaned_up_without_crash() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

        let override_data = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Butt),
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Insulation".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::OuterFace,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

        // Remove the Insulation layer entirely from wall A's style.
        write_wall_layers(&mut scene, wall_a, vec![wl("Concrete", 0.2, "Structural")]);

        let (kind, _touched) = join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("regeneration must not fail even though a referenced layer is gone");
        assert_eq!(kind, JoinKind::L);

        let cleaned = read_junction_override(&scene, wall_a, 1)
            .expect("default_style should survive the cleanup");
        assert!(cleaned.layer_pairs.is_empty());

        let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
            .expect("wall A should still read back as WALL");
        assert!(
            !wall_a_v2.derived_handles.is_empty(),
            "wall A should still have a fallback footprint after cleanup"
        );
    }

    /// Step 3: an override with only an (invalidated) `layer_pairs` entry and
    /// no `default_style` must have its XDATA tag fully erased once cleanup
    /// leaves nothing meaningful behind.
    #[test]
    fn fully_invalid_override_removes_xdata_tag_entirely() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

        write_wall_layers(
            &mut scene,
            wall_a,
            wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
        );

        join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("regeneration must succeed via automatic fallback");

        assert_eq!(
            read_junction_override(&scene, wall_a, 1),
            None,
            "the degenerate override should be erased entirely, not left as an empty record"
        );
    }

    /// Step 3: when a wall at an N-way junction is deleted, another wall's
    /// override that referenced one of the deleted wall's layers as
    /// `layer_b` must not cause a panic on the next regeneration of the
    /// remaining walls — it is cleaned up gracefully instead.
    #[test]
    fn deleted_wall_at_junction_does_not_panic_remaining_override() {
        fn add_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("Concrete", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        }
        let mut scene = Scene::new();
        // Three walls meeting at the origin (X-ish junction).
        let w1 = add_wall(&mut scene, (0.0, 0.0), (-10.0, 0.0));
        let w2 = add_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
        let w3 = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));

        for h in [w1, w2, w3] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene, &[w1, w2, w3], None, None, None, None).expect("initial N-way join");

        // W1 stores an override whose `layer_b` names W3's Concrete layer.
        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: Some(join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                }),
                style: join::JoinOverrideStyle::Butt,
            }],
        };
        let end_1 = if get_wall_vertices(&scene, w1)[0].distance(DVec3::ZERO) < 1e-6 {
            0
        } else {
            1
        };
        assert!(write_junction_override(&mut scene, w1, end_1, &override_data));

        // Delete W3 entirely from the document.
        scene.document.remove_entity(w3);

        // Re-resolving the junction with only the remaining walls must not
        // panic, even though W1's override still references the deleted
        // wall's layer.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            join_junction_in_document(&mut scene, &[w1, w2], None, None, None, None)
        }));
        assert!(result.is_ok(), "regeneration must not panic after a peer wall was deleted");
    }

    /// Step 3 regression: a still-valid override (referenced layer/material
    /// unchanged) must not be touched by the invalidation pass.
    #[test]
    fn valid_override_is_not_touched_by_invalidation() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));

        let _ = take_pending_override_warnings();
        join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None).expect("join should succeed");

        assert_eq!(
            read_junction_override(&scene, wall_a, 1),
            Some(override_data),
            "a still-valid override must survive regeneration unchanged"
        );
        assert!(
            take_pending_override_warnings().is_empty(),
            "no invalidation notice should be queued for a valid override"
        );
    }

    /// Step 3: the user-visible notice mechanism (`take_pending_override_warnings`,
    /// drained via `command_line.push_info` at command entry points) must
    /// actually be invoked when an override is invalidated and removed.
    #[test]
    fn invalidated_override_queues_and_surfaces_a_notice() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("wall A regen");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("wall B regen");

        let override_data = join::JunctionOverride {
            default_style: None,
            layer_pairs: vec![join::LayerPairOverride {
                layer_a: join::LayerRef {
                    material_id: "Concrete".to_string(),
                    role_tag: None,
                    index: 0,
                layer_id: None,
                },
                layer_b: None,
                style: join::JoinOverrideStyle::NoExtend,
            }],
        };
        assert!(write_junction_override(&mut scene, wall_a, 1, &override_data));
        write_wall_layers(
            &mut scene,
            wall_a,
            wls(&[("Brick", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]),
        );

        let _ = take_pending_override_warnings(); // drain any leftovers from prior tests
        let mut command_line = CommandLine::default();
        aec_walljoin_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|{}", wall_a.value(), wall_b.value()), 
        None,  None,  None);

        assert!(
            command_line
                .history
                .iter()
                .any(|e| e.text.contains("join override") || e.text.contains("Verbindungsüberschreibung")),
            "the command line should surface an invalidation notice, got {:?}",
            command_line.history.iter().map(|e| &e.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn regenerate_wall_representation_builds_layers_and_avoids_duplication() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let before = scene.document.entities().count();

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");
        let after_first = scene.document.entities().count();
        assert!(
            after_first > before,
            "regeneration should have created new contour/hatch/solid entities"
        );

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("wall should still be readable as WALL after regeneration");
        let derived_after_first = wall.derived_handles.clone();
        assert!(!derived_after_first.is_empty());

        // Calling it again must replace, not accumulate, the derived
        // entities.
        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("second regeneration should also succeed");
        let after_second = scene.document.entities().count();
        assert_eq!(
            after_first, after_second,
            "regenerating twice should not duplicate derived entities"
        );

        let wall2 = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("wall should still be readable as WALL after second regeneration");
        assert_eq!(wall2.derived_handles.len(), derived_after_first.len());
    }

    #[test]
    fn regenerate_wall_representation_respects_layer_override() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut overridden = wl("Concrete", 0.2, "Structural");
        overridden.layer_override = Some("AEC_OVERRIDE_LAYER".to_string());
        let default_layer = wl("Insulation", 0.05, "Insulation");
        let layers = vec![overridden, default_layer];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed for a valid two-layer wall");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .expect("should still read back as WALL")
            .derived_handles;
        assert!(!derived.is_empty());

        let mut saw_override_layer = false;
        let mut saw_default_layer = false;
        for h in derived {
            let e = scene.document.get_entity(h).unwrap();
            if e.common().layer == "AEC_OVERRIDE_LAYER" {
                saw_override_layer = true;
            } else if e.common().layer == "0" || !e.common().layer.is_empty() {
                saw_default_layer = true;
            }
        }
        assert!(
            saw_override_layer,
            "at least one derived entity of the overridden layer should be placed on AEC_OVERRIDE_LAYER"
        );
        assert!(
            saw_default_layer,
            "the non-overridden layer's derived entities should keep using the default layer"
        );
        assert!(scene.document.layers.contains("AEC_OVERRIDE_LAYER"));
    }

    #[test]
    fn regenerate_wall_representation_with_rules_none_matches_default_behavior() {
        use crate::modules::aec::engine::display_component::ComponentRuleSet;

        let mut scene_plain = Scene::new();
        let wall_plain = add_multi_layer_wall(&mut scene_plain);
        regenerate_wall_representation(&mut scene_plain, wall_plain, None)
            .expect("plain regeneration should succeed");
        let derived_plain = wall_from_entity(scene_plain.document.get_entity(wall_plain).unwrap())
            .unwrap()
            .derived_handles;

        let mut scene_none = Scene::new();
        let wall_none = add_multi_layer_wall(&mut scene_none);
        regenerate_wall_representation_with_rules(&mut scene_none, wall_none, None, None)
            .expect("rules-aware regeneration with None should succeed");
        let derived_none = wall_from_entity(scene_none.document.get_entity(wall_none).unwrap())
            .unwrap()
            .derived_handles;
        assert_eq!(derived_plain.len(), derived_none.len());

        let mut scene_default = Scene::new();
        let wall_default = add_multi_layer_wall(&mut scene_default);
        let default_rules = ComponentRuleSet::default();
        regenerate_wall_representation_with_rules(
            &mut scene_default,
            wall_default,
            Some(&default_rules),
        None)
        .expect("rules-aware regeneration with default (all-visible) rules should succeed");
        let derived_default =
            wall_from_entity(scene_default.document.get_entity(wall_default).unwrap())
                .unwrap()
                .derived_handles;
        assert_eq!(
            derived_plain.len(),
            derived_default.len(),
            "a default ComponentRuleSet (everything visible) must reproduce today's behavior"
        );
    }

    #[test]
    fn regenerate_wall_representation_with_rules_hides_solid3d_slot() {
        use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Solid3D.key().to_string(), false);

        regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
            .expect("regeneration with Solid3D hidden should still succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;
        assert!(!derived.is_empty());

        let mut saw_contour = false;
        let mut saw_hatch = false;
        let mut saw_solid = false;
        for h in &derived {
            match scene.document.get_entity(*h) {
                Some(EntityType::LwPolyline(_)) => saw_contour = true,
                Some(EntityType::Hatch(_)) => saw_hatch = true,
                Some(EntityType::Solid3D(_)) => saw_solid = true,
                _ => {}
            }
        }
        assert!(saw_contour, "contours should remain when only Solid3D is hidden");
        assert!(saw_hatch, "hatches should remain when only Solid3D is hidden");
        assert!(!saw_solid, "no Solid3D entity should be created when the slot is hidden");
    }

    #[test]
    fn regenerate_wall_representation_with_rules_hides_layers2d_slot() {
        use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Layers2D.key().to_string(), false);
        rules
            .visibility
            .insert(WallComponentSlot::Contour2D.key().to_string(), false);

        regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
            .expect("regeneration with 2D contour slots hidden should still succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;
        assert!(!derived.is_empty());

        let mut saw_contour = false;
        let mut saw_solid = false;
        for h in &derived {
            match scene.document.get_entity(*h) {
                Some(EntityType::LwPolyline(_)) => saw_contour = true,
                Some(EntityType::Solid3D(_)) => saw_solid = true,
                _ => {}
            }
        }
        assert!(
            !saw_contour,
            "no contour polyline should be created when Layers2D and Contour2D are hidden"
        );
        assert!(saw_solid, "solids should remain when only 2D contour slots are hidden");
    }

    /// Serializes access to the on-disk AEC style library file (see
    /// `engine::library::default_library_path`) for tests that need
    /// `load_or_seed()` inside `regenerate_wall_representation_inner` to see
    /// specific wall styles/materials (Step 3 `StyleSubstitution` tests):
    /// writes `lib`, runs `f`, then restores whatever was on disk before.
    fn with_test_library<F: FnOnce()>(lib: &crate::modules::aec::engine::library::StyleLibrary, f: F) {
        static LIBRARY_TEST_LOCK: Mutex<()> = Mutex::new(());
        let _guard = LIBRARY_TEST_LOCK.lock().unwrap();
        let path = crate::modules::aec::engine::library::default_library_path();
        let backup = std::fs::read_to_string(&path).ok();
        crate::modules::aec::engine::library::save_to_default_path(lib)
            .expect("failed to write test library");
        f();
        match backup {
            Some(content) => {
                let _ = std::fs::write(&path, content);
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    #[test]
    fn component_rule_set_style_override_wins_for_hatch_slot() {
        use crate::modules::aec::engine::display_component::{
            ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
        };

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let mut rules = ComponentRuleSet::default();
        rules.style_override.insert(
            WallComponentSlot::ContourHatch2D.key().to_string(),
            ComponentStyleOverride {
                hatch_pattern: Some("NET".to_string()),
                hatch_color: Some(acadrust::types::Color::Rgb { r: 0, g: 255, b: 0 }),
                ..Default::default()
            },
        );

        regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
            .expect("regeneration with a ContourHatch2D style_override should succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;

        let mut saw_overridden_pattern = false;
        let mut saw_overridden_color = false;
        for h in &derived {
            if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                if hatch.pattern.name == "NET" {
                    saw_overridden_pattern = true;
                }
                // The default fallback pattern must never appear once the
                // slot-wide override is active.
                assert_ne!(hatch.pattern.name, "ANSI31");
                if hatch.common.color
                    == (acadrust::types::Color::Rgb {
                        r: 0,
                        g: 255,
                        b: 0,
                    })
                {
                    saw_overridden_color = true;
                }
            }
        }
        assert!(
            saw_overridden_pattern,
            "every layer's hatch should use the ContourHatch2D style_override pattern"
        );
        assert!(
            saw_overridden_color,
            "hatch entities must keep the override hatch_color on the DXF entity"
        );
    }

    #[test]
    fn layer_selection_explicit_filters_contour_and_solid_to_referenced_layers() {
        use crate::modules::aec::engine::display_component::{
            ComponentRuleSet, LayerSelection, WallComponentSlot,
        };
        use crate::modules::aec::engine::join::LayerRef;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        // add_multi_layer_wall's layers are ["Concrete" (index 0), "Insulation" (index 1)].
        let mut rules = ComponentRuleSet::default();
        let explicit = LayerSelection::Explicit(vec![LayerRef {
            material_id: "Concrete".to_string(),
            role_tag: None,
            index: 0,
        layer_id: None,
        }]);
        rules
            .layer_filter
            .insert(WallComponentSlot::Contour2D.key().to_string(), explicit.clone());
        rules
            .layer_filter
            .insert(WallComponentSlot::Solid3D.key().to_string(), explicit);

        regenerate_wall_representation_with_rules(&mut scene, wall_handle, Some(&rules), None)
            .expect("regeneration with an explicit layer_filter should succeed");

        let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
            .unwrap()
            .derived_handles;

        let mut contour_count = 0usize;
        let mut solid_count = 0usize;
        let mut hatch_count = 0usize;
        for h in &derived {
            match scene.document.get_entity(*h) {
                Some(EntityType::LwPolyline(_)) => contour_count += 1,
                Some(EntityType::Solid3D(_)) => solid_count += 1,
                Some(EntityType::Hatch(_)) => hatch_count += 1,
                _ => {}
            }
        }
        assert_eq!(
            contour_count, 1,
            "only the explicitly referenced layer's contour should be created"
        );
        assert_eq!(
            solid_count, 1,
            "only the explicitly referenced layer's solid should be created"
        );
        // `layer_filter` doesn't gate hatches (plan Step 3 scope is
        // Contour2D/Solid3D only) — both layers' hatches remain.
        assert_eq!(
            hatch_count, 2,
            "layer_filter must not affect hatch creation, only Contour2D/Solid3D"
        );
    }

    #[test]
    fn style_substitution_swaps_hatch_look_but_keeps_axis_and_thickness() {
        use crate::modules::aec::engine::library::StyleLibrary;
        use crate::modules::aec::engine::material::Material;

        let source_material = Material::new(
            "SourceMat".to_string(),
            "Source".to_string(),
            "ANSI31".to_string(),
            0x111111,
            "Continuous".to_string(),
        );
        let mut target_material = Material::new(
            "TargetMat".to_string(),
            "Target".to_string(),
            "ANSI37".to_string(),
            0x222222,
            "Continuous".to_string(),
        );
        target_material.hatch_color = Some(acadrust::types::Color::Rgb { r: 171, g: 205, b: 239 });

        let source_style = WallStyle {
            style: Style {
                id: "src-style".to_string(),
                name: "Source Style".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "SourceMat".to_string(),
                thickness: LayerValue::Fixed(0.2),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        let target_style = WallStyle {
            style: Style {
                id: "tgt-style".to_string(),
                name: "Target Style".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            // Same total thickness as `source_style` (0.2), satisfying the
            // `validate_style_substitution` consistency requirement.
            layers: vec![Layer {
                material_id: "TargetMat".to_string(),
                thickness: LayerValue::Fixed(0.2),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        assert!(crate::modules::aec::engine::display_component::validate_style_substitution(
            &source_style,
            &target_style
        )
        .is_ok());

        let lib = StyleLibrary {
            materials: vec![source_material, target_material],
            wall_styles: vec![source_style, target_style],
        };

        with_test_library(&lib, || {
            let mut scene = Scene::new();
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
            pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("SourceMat", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values =
                wall_record("src-style", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            let wall_handle = scene.add_entity(entity);

            let axis_before = get_wall_vertices(&scene, wall_handle);

            let mut substitutions: HashMap<String, String> = HashMap::new();
            substitutions.insert("src-style".to_string(), "tgt-style".to_string());

            regenerate_wall_representation_with_rules_and_substitutions(
                &mut scene,
                wall_handle,
                None,
                Some(&substitutions),
            None)
            .expect("regeneration with a style substitution should succeed");

            let axis_after = get_wall_vertices(&scene, wall_handle);
            assert_eq!(axis_before, axis_after, "axis geometry must stay unchanged");
            let wall_after = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
                .expect("still a WALL");
            assert!((wall_after.layers[0].thickness - 0.2).abs() < 1e-9);

            let mut saw_target_pattern = false;
            for h in &wall_after.derived_handles {
                if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                    if hatch.pattern.name == "ANSI37" {
                        saw_target_pattern = true;
                    }
                    assert_ne!(
                        hatch.pattern.name, "ANSI31",
                        "the substituted wall must not use the source style's hatch pattern"
                    );
                }
            }
            assert!(
                saw_target_pattern,
                "the substituted wall should use the target wall style's hatch pattern"
            );
        });
    }

    #[test]
    fn detailed_style_override_wins_over_style_substitution() {
        use crate::modules::aec::engine::display_component::{
            ComponentRuleSet, ComponentStyleOverride, WallComponentSlot,
        };
        use crate::modules::aec::engine::library::StyleLibrary;
        use crate::modules::aec::engine::material::Material;

        let source_material = Material::new(
            "SourceMat2".to_string(),
            "Source".to_string(),
            "ANSI31".to_string(),
            0x111111,
            "Continuous".to_string(),
        );
        let target_material = Material::new(
            "TargetMat2".to_string(),
            "Target".to_string(),
            "ANSI37".to_string(),
            0x222222,
            "Continuous".to_string(),
        );
        let source_style = WallStyle {
            style: Style {
                id: "src-style-2".to_string(),
                name: "Source Style 2".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "SourceMat2".to_string(),
                thickness: LayerValue::Fixed(0.2),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };
        let target_style = WallStyle {
            style: Style {
                id: "tgt-style-2".to_string(),
                name: "Target Style 2".to_string(),
                object_kind: "Wall".to_string(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "TargetMat2".to_string(),
                thickness: LayerValue::Fixed(0.2),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        };

        let lib = StyleLibrary {
            materials: vec![source_material, target_material],
            wall_styles: vec![source_style, target_style],
        };

        with_test_library(&lib, || {
            let mut scene = Scene::new();
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
            pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("SourceMat2", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record(
                "src-style-2",
                3.0,
                0,
                &layers,
                &[],
                WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            let wall_handle = scene.add_entity(entity);

            let mut substitutions: HashMap<String, String> = HashMap::new();
            substitutions.insert("src-style-2".to_string(), "tgt-style-2".to_string());

            let mut rules = ComponentRuleSet::default();
            rules.style_override.insert(
                WallComponentSlot::ContourHatch2D.key().to_string(),
                ComponentStyleOverride {
                    hatch_pattern: Some("NET".to_string()),
                    ..Default::default()
                },
            );

            regenerate_wall_representation_with_rules_and_substitutions(
                &mut scene,
                wall_handle,
                Some(&rules),
                Some(&substitutions),
            None)
            .expect("regeneration with both a Detailed override and an applicable substitution should succeed");

            let derived = wall_from_entity(scene.document.get_entity(wall_handle).unwrap())
                .unwrap()
                .derived_handles;

            let mut saw_detailed_pattern = false;
            for h in &derived {
                if let Some(EntityType::Hatch(hatch)) = scene.document.get_entity(*h) {
                    assert_eq!(
                        hatch.pattern.name, "NET",
                        "the Detailed style_override must win over the style substitution's target pattern"
                    );
                    saw_detailed_pattern = true;
                }
            }
            assert!(saw_detailed_pattern, "a hatch should have been created");
        });
    }

    #[test]
    fn regenerate_wall_representation_with_rules_hides_slot_at_joined_corner() {
        use crate::modules::aec::engine::display_component::{ComponentRuleSet, WallComponentSlot};

        // Two joined walls sharing a corner via `regenerate_wall_representation_with_corner_and_rules`,
        // confirming the Solid3D override still applies at a mitered/extended corner.
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = wls(&[("Concrete", 0.2, "Structural"), ("Insulation", 0.05, "Insulation")]);
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Solid3D.key().to_string(), false);

        // Regenerate wall_a with a corner-extension override toward wall_b's
        // start vertex, same shape as the plain join path uses, but with the
        // Solid3D slot hidden.
        regenerate_wall_representation_with_corner_and_rules(
            &mut scene,
            wall_a,
            Some((1, DVec3::new(5.0, 0.0, 0.0))),
            None,
            Some(&rules),
        None)
        .expect("joined-corner regeneration with Solid3D hidden should still succeed");

        let derived_a = wall_from_entity(scene.document.get_entity(wall_a).unwrap())
            .unwrap()
            .derived_handles;
        assert!(!derived_a.is_empty());
        let mut saw_contour = false;
        let mut saw_solid = false;
        for h in &derived_a {
            match scene.document.get_entity(*h) {
                Some(EntityType::LwPolyline(_)) => saw_contour = true,
                Some(EntityType::Solid3D(_)) => saw_solid = true,
                _ => {}
            }
        }
        assert!(saw_contour, "contours should still be produced at the joined corner");
        assert!(!saw_solid, "Solid3D should stay hidden at the joined corner too");

        let _ = wall_b; // kept alive to represent the join partner
    }

    #[test]
    fn wall_axis_still_detected_by_room_loop_after_regeneration() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let corners = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(4.0, 0.0, 0.0),
            DVec3::new(4.0, 3.0, 0.0),
            DVec3::new(0.0, 3.0, 0.0),
            DVec3::new(0.0, 0.0, 0.0),
        ];

        let mut handles = Vec::new();
        for pair in corners.windows(2) {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(pair[0].x, pair[0].y)));
            pl.add_vertex(LwVertex::new(Vector2::new(pair[1].x, pair[1].y)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("Concrete", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("style1", 2.8, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            handles.push(scene.add_entity(entity));
        }

        // Regenerate every wall: this moves each axis polyline onto the
        // dedicated AEC_WALL_AXIS layer.
        for h in &handles {
            regenerate_wall_representation(&mut scene, *h, None)
                .expect("regeneration should succeed for every wall segment");
        }
        for h in &handles {
            let e = scene.document.get_entity(*h).unwrap();
            assert_eq!(e.common().layer, AEC_WALL_AXIS_LAYER);
        }

        // AEC_ROOM / collect_wall_segments must still see the closed loop.
        let segments = collect_wall_segments(&scene.document);
        assert_eq!(segments.len(), 4);

        let mut command_line = CommandLine::default();
        aec_room(&mut scene, &mut command_line);
        let room_record = scene
            .document
            .entities()
            .filter_map(read_aec_record)
            .find(|r| matches!(r.values.first(), Some(XDataValue::String(k)) if k == "ROOM"))
            .expect("aec_room should have written a ROOM xdata record");
        let area = match room_record.values.get(2) {
            Some(XDataValue::Real(a)) => *a,
            _ => panic!("ROOM record should carry an area value"),
        };
        assert!((area - 12.0).abs() < 1e-6);
    }

    #[test]
    fn wall_layer_extrusions_calculates_offsets_and_effective_height() {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let entity = EntityType::LwPolyline(pl);
        let layers = vec![WallLayer {
            material: "brick".to_string(),
            thickness: 0.2,
            function: "Structural".to_string(),
            axis_offset: -0.1,
            bottom_offset: 0.5,
            top_offset: 0.3,
            layer_override: None,
            hatch_override: None,
        layer_id: uuid::Uuid::new_v4(),
        }];
        let height = 3.0;
        let extrusions = wall_layer_extrusions(&entity, &layers, height);
        assert_eq!(extrusions.len(), 1);
        assert!((extrusions[0].height - 2.2).abs() < 1e-9); // 3.0 - 0.5 - 0.3 = 2.2
        assert!((extrusions[0].base_offset - 0.5).abs() < 1e-9);
    }

    #[test]
    fn regenerate_wall_representation_respects_vertical_offsets() {
        use acadrust::types::Vector2;
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut layer = wl("Concrete", 0.2, "Structural");
        layer.bottom_offset = 0.5;
        layer.top_offset = 0.3;
        let layers = vec![layer];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();

        let mut saw_bottom_z = false;
        let mut saw_top_z = false;
        for h in wall.derived_handles {
            let e = scene.document.get_entity(h).unwrap();
            if let EntityType::Solid3D(s3d) = e {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        if (pt.z - 0.5).abs() < 1e-9 {
                            saw_bottom_z = true;
                        }
                        if (pt.z - 2.7).abs() < 1e-9 {
                            saw_top_z = true;
                        }
                    }
                }
            }
        }
        assert!(saw_bottom_z, "solid should have wires at Z=0.5");
        assert!(saw_top_z, "solid should have wires at Z=2.7");
    }

    #[test]
    fn wall_regeneration_reflects_updated_layers() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);

        let initial_layers = vec![wl("Brick", 0.1, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values =
            wall_record("style1", 3.0, 0, &initial_layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        // Initial regeneration
        regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

        // Update layers
        let updated_layers = vec![wl("Brick", 0.5, "Structural")];
        assert!(write_wall_layers(&mut scene, wall_handle, updated_layers));

        // Regenerate again
        regenerate_wall_representation(&mut scene, wall_handle, None).unwrap();

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        assert_eq!(wall.layers[0].thickness, 0.5);

        // Check geometry (thickness is reflected in Solid3D width)
        let mut max_y = 0.0;
        for h in wall.derived_handles {
            let e = scene.document.get_entity(h).unwrap();
            if let EntityType::Solid3D(s3d) = e {
                for wire in &s3d.wires {
                    for pt in &wire.points {
                        if pt.y.abs() > max_y {
                            max_y = pt.y.abs();
                        }
                    }
                }
            }
        }
        // For Center justification, half of 0.5 should be at Y=0.25 and Y=-0.25
        assert!((max_y - 0.25).abs() < 1e-6);
    }

    #[test]
    fn aec_wallextend_do_extends_to_an_explicit_point() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
        let mut command_line = CommandLine::default();

        aec_wallextend_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|PT|8|0|0", wall_handle.value()), 
        None,  None,  None);

        let axis = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis.len(), 2);
        // The nearer endpoint (5,0) should have moved to the target (8,0);
        // the far endpoint (0,0) stays put.
        assert!(axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
        assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));

        // Both the 2D (contour/hatch) and 3D (solid) derived representation
        // must reflect the new, extended axis length — not just the axis
        // polyline itself.
        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        let mut max_x_2d: f64 = 0.0;
        let mut max_x_3d: f64 = 0.0;
        for h in &wall.derived_handles {
            match scene.document.get_entity(*h).unwrap() {
                EntityType::LwPolyline(pl) => {
                    for v in &pl.vertices {
                        max_x_2d = max_x_2d.max(v.location.x);
                    }
                }
                EntityType::Solid3D(s3d) => {
                    for wire in &s3d.wires {
                        for pt in &wire.points {
                            max_x_3d = max_x_3d.max(pt.x as f64);
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(
            max_x_2d > 7.9,
            "2D contour should extend to the new endpoint, got max_x={max_x_2d}"
        );
        assert!(
            max_x_3d > 7.9,
            "3D solid should extend to the new endpoint, got max_x={max_x_3d}"
        );
    }

    #[test]
    fn aec_wallextend_do_preserves_direction_for_an_off_line_target() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // axis: (0,0) -> (5,0)
        let mut command_line = CommandLine::default();

        // Target point is NOT collinear with the wall's (0,0)->(5,0) axis
        // (it has a non-zero Y). Extending must not bend the wall towards
        // this point — it must project onto the original direction line.
        aec_wallextend_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|PT|8|2|0", wall_handle.value()), 
        None,  None,  None);

        let axis = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis.len(), 2);
        // Original direction was purely along +X (Y=0 for both points);
        // the new endpoint must keep the same direction, i.e. still Y=0.
        assert!(
            axis.iter().any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9),
            "extended endpoint should stay on the original direction line, got {:?}",
            axis
        );
        assert!(axis.iter().any(|p| p.x.abs() < 1e-9 && p.y.abs() < 1e-9));
    }

    #[test]
    fn aec_wallextend_do_preserves_direction_for_a_multi_vertex_bent_wall() {
        // Regression test for a real-world root cause: for a multi-vertex
        // (bent) wall polyline, the direction to preserve when extending an
        // endpoint must come from the segment immediately adjacent to that
        // endpoint, NOT from a line drawn to the opposite far end of the
        // whole polyline (which, for a bent wall, points in a different
        // direction and would visibly change the extended segment's angle).
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        // A bent, 3-vertex wall axis: (0,0) -> (5,0) -> (5,5).
        // The last segment (5,0)->(5,5) runs purely along +Y.
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall_handle = scene.add_entity(entity);

        let mut command_line = CommandLine::default();

        // Extend the (5,5) endpoint further along +Y, to (5,8).
        aec_wallextend_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|PT|5|8|0", wall_handle.value()), 
        None,  None,  None);

        let axis = get_wall_vertices(&scene, wall_handle);
        assert_eq!(axis.len(), 3);
        // The extended endpoint must stay on the last segment's direction
        // (X=5), not bend towards the far opposite end (0,0).
        let last = axis.last().unwrap();
        assert!(
            (last.x - 5.0).abs() < 1e-9 && (last.y - 8.0).abs() < 1e-9,
            "extended endpoint should stay on the adjacent segment's direction line, got {:?}",
            axis
        );
    }

    #[test]
    fn aec_wallextend_do_extends_to_intersect_another_wall() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        // Wall A: (0,0) -> (5,0), stopping short of Wall B's axis.
        let wall_a = add_multi_layer_wall(&mut scene);
        // Wall B: a "through" wall running vertically at x=8, so extending A
        // towards it produces a T-junction trim on A only.
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record(
            "style1",
            3.0,
            0,
            &vec![wl("Concrete", 0.2, "Structural")],
            &[],
            WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        let mut command_line = CommandLine::default();
        aec_wallextend_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
        None,  None,  None);

        let axis_a = get_wall_vertices(&scene, wall_a);
        // Wall A should now end exactly at the intersection with wall B's axis.
        assert!(axis_a
            .iter()
            .any(|p| (p.x - 8.0).abs() < 1e-6 && p.y.abs() < 1e-6));
        // Wall B (the "through" wall) stays unchanged.
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert_eq!(axis_b.len(), 2);
        assert!((axis_b[0].y - (-5.0)).abs() < 1e-9);
        assert!((axis_b[1].y - 5.0).abs() < 1e-9);
    }

    /// Regression test for the reported bug: `AEC_WALLEXTEND` to a target
    /// wall visually trims/miters the extended wall, but never registered
    /// the two walls as joined peers (`JOINED_PEERS` via
    /// `engine::owner_index::link_peers`) — unlike `AEC_WALLJOIN` and the
    /// automatic join performed while drawing. Without that peer link, the
    /// connection isn't recognized as a real join by later operations (e.g.
    /// re-resolving the junction after a subsequent move), so it appears as
    /// if "no join was created".
    #[test]
    fn aec_wallextend_do_links_peers_with_target_wall() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene); // (0,0) -> (5,0)
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record(
            "style1",
            3.0,
            0,
            &vec![wl("Concrete", 0.2, "Structural")],
            &[],
            WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        let mut command_line = CommandLine::default();
        aec_wallextend_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|WALL|{}", wall_a.value(), wall_b.value()), 
        None,  None,  None);

        let peers_a = engine::owner_index::peers_of(&scene.document, wall_a);
        let peers_b = engine::owner_index::peers_of(&scene.document, wall_b);
        assert_eq!(
            peers_a,
            vec![wall_b],
            "extended wall must be linked as a peer of its target wall"
        );
        assert_eq!(
            peers_b,
            vec![wall_a],
            "target wall must be linked as a peer of the extended wall"
        );
    }

    #[test]
    fn aec_walljoin_do_forces_l_even_when_one_wall_overhangs() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add_wall = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let through = add_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let stem = add_wall(&mut scene, (5.0, 1.0), (5.0, 4.0));
        let mut command_line = CommandLine::default();
        aec_walljoin_do(
            &mut scene, 
            &mut command_line, 
            &format!("{}|{}", through.value(), stem.value()), 
        None,  None,  None);
        let axis_t = get_wall_vertices(&scene, through);
        let axis_s = get_wall_vertices(&scene, stem);
        assert!(
            axis_t
                .iter()
                .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
            "through wall must be trimmed to the L corner, got {axis_t:?}"
        );
        assert!(
            axis_s
                .iter()
                .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
            "stem must reach the L corner, got {axis_s:?}"
        );
        assert!(
            !axis_t.iter().any(|p| (p.x - 10.0).abs() < 1e-6),
            "overhang past the corner must be removed"
        );
    }

    #[test]
    fn regenerate_after_axis_shorten_matches_new_length() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene); // (0,0)-(5,0)
        let _ = regenerate_wall_representation(&mut scene, wall, None);
        update_wall_vertices(
            &mut scene,
            wall,
            &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(3.0, 0.0, 0.0)],
        );
        let _ = regenerate_wall_representation(&mut scene, wall, None);
        let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
        let mut max_x = 0.0_f64;
        for h in rec.derived_handles {
            if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
                for v in &pl.vertices {
                    max_x = max_x.max(v.location.x);
                }
            }
        }
        assert!(
            max_x < 3.2,
            "2D contour must follow the shortened axis, got max_x={max_x}"
        );
        assert!(max_x > 2.8, "2D contour should still reach the new end");
    }

    #[test]
    fn regenerate_rewrites_existing_contour_polyline() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let _ = regenerate_wall_representation(&mut scene, wall, None);
        let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
        let contour_h = rec
            .derived_handles
            .iter()
            .copied()
            .find(|&h| matches!(scene.document.get_entity(h), Some(EntityType::LwPolyline(_))))
            .expect("contour");

        update_wall_vertices(
            &mut scene,
            wall,
            &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0)],
        );
        let _ = regenerate_wall_representation(&mut scene, wall, None);

        let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
        assert!(
            rec.derived_handles.contains(&contour_h),
            "contour handle must be reused so tessellation can retessellate it"
        );
        let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(contour_h) else {
            panic!("contour still a polyline");
        };
        let max_x = pl
            .vertices
            .iter()
            .map(|v| v.location.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (max_x - 4.0).abs() < 0.3,
            "reused contour must follow new axis, max_x={max_x}"
        );
    }

    #[test]
    fn erase_wall_live_preview_companions_drops_untagged_draw_contour() {
        // Bug B / Step 3: WallCommand commits an untagged outer-contour
        // polyline for live preview. On finish it must be erased so only
        // WALL_REP children remain (which regenerate with the axis).
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);

        let mut preview = LwPolyline::new();
        preview.is_closed = true;
        preview.add_vertex(LwVertex::new(Vector2::new(0.0, -0.1)));
        preview.add_vertex(LwVertex::new(Vector2::new(10.0, -0.1)));
        preview.add_vertex(LwVertex::new(Vector2::new(10.0, 0.1)));
        preview.add_vertex(LwVertex::new(Vector2::new(0.0, 0.1)));
        let preview_h = scene.add_entity(EntityType::LwPolyline(preview));

        // A properly tagged WALL_REP child must NOT be erased by the helper.
        let mut tagged = LwPolyline::new();
        tagged.is_closed = true;
        tagged.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 0.0)));
        tagged.add_vertex(LwVertex::new(Vector2::new(1.0, 1.0)));
        let tagged_h = scene.add_entity(EntityType::LwPolyline(tagged));
        write_wall_display_tag(&mut scene, tagged_h, wall, WALL_REP_ROLE_CONTOUR);

        erase_wall_live_preview_companions(&mut scene, wall, &[preview_h, tagged_h]);

        assert!(
            scene.document.get_entity(preview_h).is_none(),
            "untagged live preview contour must be erased on wall finish"
        );
        assert!(
            scene.document.get_entity(tagged_h).is_some(),
            "WALL_REP children must not be erased by preview cleanup"
        );
        assert!(
            scene.document.get_entity(wall).is_some(),
            "wall axis must remain"
        );
    }

    #[test]
    fn regenerate_erases_orphan_wall_rep_contour() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        let _ = regenerate_wall_representation(&mut scene, wall, None);

        let mut orphan = LwPolyline::new();
        orphan.is_closed = true;
        orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 0.0)));
        orphan.add_vertex(LwVertex::new(Vector2::new(9.0, 1.0)));
        orphan.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
        let orphan_h = scene.add_entity(EntityType::LwPolyline(orphan));
        write_wall_display_tag(&mut scene, orphan_h, wall, WALL_REP_ROLE_CONTOUR);

        update_wall_vertices(
            &mut scene,
            wall,
            &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(2.0, 0.0, 0.0)],
        );
        let _ = regenerate_wall_representation(&mut scene, wall, None);

        // Orphan handle may be reused as the new contour; it must not keep
        // the old 9-unit rectangle either way.
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(orphan_h) {
            let max_x = pl
                .vertices
                .iter()
                .map(|v| v.location.x)
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                max_x < 2.3,
                "reused leftover contour must follow shortened axis, max_x={max_x}"
            );
        }
        let rec = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
        let mut max_x = 0.0_f64;
        for h in rec.derived_handles {
            if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(h) {
                for v in &pl.vertices {
                    max_x = max_x.max(v.location.x);
                }
            }
        }
        assert!(max_x < 2.3, "new contour must follow shortened axis, max_x={max_x}");
    }

    #[test]
    fn change_wall_justification_shifts_axis_by_expected_distance() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene); // total thickness 0.25
        let axis_before = get_wall_vertices(&scene, wall_handle);
        assert!(axis_before.iter().all(|p| p.y.abs() < 1e-9));

        assert!(change_wall_justification(
            &mut scene,
            wall_handle,
            WallJustification::Interior,
        None));

        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        assert_eq!(wall.justification, WallJustification::Interior);

        let axis_after = get_wall_vertices(&scene, wall_handle);
        // Center -> Interior delta is -0.5 * total_thickness = -0.125; the
        // offset direction for a straight horizontal axis is +Y, so the axis
        // should have shifted to Y = -0.125.
        for p in &axis_after {
            assert!((p.y - (-0.125)).abs() < 1e-6);
        }
    }

    #[test]
    fn wall_extend_command_auto_detects_target_wall_on_entity_pick() {
        // Once a source wall is selected, clicking another wall (without
        // typing W) must dispatch the WALL|target branch.
        let mut cmd = WallExtendCommand::new();
        assert!(cmd.needs_entity_pick());

        let source = Handle::new(10);
        let target = Handle::new(20);
        assert!(matches!(
            cmd.on_entity_pick(source, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(cmd.needs_entity_pick());

        match cmd.on_entity_pick(target, DVec3::new(8.0, 0.0, 0.0)) {
            CmdResult::Dispatch(s) => {
                assert_eq!(s, format!("AEC_WALLEXTEND_DO {}|WALL|{}", source.value(), target.value()));
            }
            _ => panic!("expected WALL| dispatch"),
        }
    }

    #[test]
    fn wall_extend_command_empty_click_stays_in_to_wall_mode() {
        // A null-handle pick (empty space) after the source wall is selected
        // must NOT fall back to a point extend in the default `ToWall`
        // mode — the user must pick a different wall or switch to `Point`
        // mode explicitly.
        let mut cmd = WallExtendCommand::new();
        let source = Handle::new(10);
        let _ = cmd.on_entity_pick(source, DVec3::ZERO);

        assert!(matches!(
            cmd.on_entity_pick(Handle::NULL, DVec3::new(8.0, 2.0, 0.0)),
            CmdResult::NeedPoint
        ));
    }

    #[test]
    fn wall_extend_command_point_mode_dispatches_pt_after_switch() {
        // Typing `P` switches to `ToPoint` mode; a subsequent point pick
        // must dispatch the PT| point-projection path.
        let mut cmd = WallExtendCommand::new();
        let source = Handle::new(10);
        let _ = cmd.on_entity_pick(source, DVec3::ZERO);
        assert!(matches!(cmd.on_text_input("P"), Some(CmdResult::NeedPoint)));
        assert!(!cmd.needs_entity_pick());

        match cmd.on_point(DVec3::new(8.0, 2.0, 0.0)) {
            CmdResult::Dispatch(s) => {
                assert!(
                    s.starts_with(&format!("AEC_WALLEXTEND_DO {}|PT|", source.value())),
                    "expected PT| dispatch, got {s}"
                );
            }
            _ => panic!("expected PT| dispatch after switching to Point mode"),
        }
    }

    #[test]
    fn regenerate_wall_representation_returns_axis_and_derived_handles() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);

        let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regeneration should succeed");

        assert!(
            touched.contains(&wall_handle),
            "returned set must include the axis handle"
        );
        let wall = wall_from_entity(scene.document.get_entity(wall_handle).unwrap()).unwrap();
        assert!(
            !wall.derived_handles.is_empty(),
            "regeneration should create derived entities"
        );
        for h in &wall.derived_handles {
            assert!(
                touched.contains(h),
                "returned set must include derived handle {}",
                h.value()
            );
        }
        // Axis + every derived.
        assert_eq!(touched.len(), 1 + wall.derived_handles.len());
    }

    /// Regression for the Properties-panel/vertex-edit staleness bug: after
    /// moving a wall's axis vertex and regenerating its representation, the
    /// *resident* (GPU-facing) wire set — the one `invalidate_property_targets`
    /// feeds via `bump_entities` — must reflect the new contour geometry, not
    /// a leftover outline from before the edit. `refresh_wall_after_axis_edit`
    /// forces this via `scene.bump_geometry()`; any other axis-edit caller must
    /// reach the same end state.
    #[test]
    fn wall_axis_edit_updates_resident_contour_wires() {
        use crate::scene::view::camera::Camera;

        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall_handle, None).expect("initial regen");

        // Prime the resident (camera-independent, GPU-facing) wire cache at
        // the original axis position.
        let cam = Camera::default();
        let _ = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);

        // Simulate a Properties-panel vertex edit: move the wall's endpoint
        // far away, regenerate, then invalidate exactly like
        // `invalidate_property_targets` does today — only `bump_entities` on
        // the touched handles, no `bump_geometry()`.
        let new_vertices = vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(500.0, 0.0, 0.0)];
        update_wall_vertices(&mut scene, wall_handle, &new_vertices);
        let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("regen after vertex edit");
        let changes: Vec<_> = touched
            .iter()
            .map(|&h| (h, crate::scene::ChangeKind::Modified))
            .collect();
        scene.bump_entities(&changes);

        // The resident wire set must no longer contain any wire endpoint at
        // the old axis extent (x == 5.0); every wall-derived wire must reach
        // out to the new extent (x == 500.0).
        let wires = scene.model_tile_wires_arc(0, &cam, 1.0, 1.0);
        let mut saw_new_extent = false;
        for wire in wires.iter() {
            let Some(handle) = Scene::handle_from_wire_name(&wire.name) else {
                continue;
            };
            if !touched.contains(&handle) {
                continue;
            }
            for pt in &wire.points {
                if pt[0].is_nan() {
                    // Tombstone slot from the resident-wire splice; not real
                    // geometry.
                    continue;
                }
                assert!(
                    (pt[0] - 5.0).abs() > 1e-6,
                    "resident wire for handle {} still shows the pre-edit contour \
                     at x=5.0 (stale tessellation); point={:?}",
                    handle.value(),
                    pt
                );
                if (pt[0] - 500.0).abs() < 1e-6 {
                    saw_new_extent = true;
                }
            }
        }
        assert!(
            saw_new_extent,
            "resident wire set never reached the new axis extent (x=500.0); \
             contour/hatch did not visibly follow the moved wall point"
        );
    }

    /// Regression for the STRETCH bug: the wall axis lives on the invisible
    /// `AEC_WALL_AXIS` layer, so a crossing-window stretch only ever sees the
    /// visible contour handle. Moving that contour's own vertices in place
    /// (the naive/buggy approach) is immediately reverted by the next
    /// `refresh_wall_after_axis_edit` regeneration, because it rebuilds the
    /// contour from the *unchanged* axis. Only moving the axis itself, then
    /// regenerating, actually relocates the visible wall — this is exactly
    /// the fix applied to `CmdResult::StretchEntities` in
    /// `command_driver.rs`.
    #[test]
    fn stretching_only_the_contour_is_reverted_by_regen_but_stretching_the_axis_sticks() {
        let mut scene = Scene::new();
        let wall_handle = add_multi_layer_wall(&mut scene);
        let touched = regenerate_wall_representation(&mut scene, wall_handle, None)
            .expect("initial regen");

        let contour_handle = *touched
            .iter()
            .find(|&&h| {
                h != wall_handle
                    && matches!(
                        scene.document.get_entity(h),
                        Some(EntityType::LwPolyline(pl)) if pl.is_closed
                    )
            })
            .expect("wall must have produced a closed contour");

        // --- Buggy path: mutate only the visible contour's vertices in
        // place (what STRETCH's generic LwPolyline branch used to do for a
        // wall's contour handle before the fix), then regenerate.
        let mut contour_before = match scene.document.get_entity(contour_handle) {
            Some(EntityType::LwPolyline(pl)) => pl.clone(),
            _ => panic!("expected contour LwPolyline"),
        };
        for v in &mut contour_before.vertices {
            v.location.x += 495.0;
        }
        update_wall_vertices(&mut scene, contour_handle, &[]); // no-op guard; contour isn't the axis
        if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity_mut(contour_handle) {
            *pl = contour_before;
        }
        let reverted = refresh_wall_after_axis_edit(&mut scene,  wall_handle,  None,  None,  None);
        let still_short = get_wall_vertices(&scene, wall_handle)
            .iter()
            .all(|v| v.x < 400.0);
        assert!(
            still_short,
            "regenerating from the untouched axis must revert a contour-only stretch \
             (this is the bug being fixed): axis vertices = {:?}",
            get_wall_vertices(&scene, wall_handle)
        );
        let contour_after_revert = reverted
            .iter()
            .find(|&&h| {
                matches!(
                    scene.document.get_entity(h),
                    Some(EntityType::LwPolyline(pl)) if pl.is_closed
                )
            })
            .and_then(|&h| match scene.document.get_entity(h) {
                Some(EntityType::LwPolyline(pl)) => Some(pl.clone()),
                _ => None,
            })
            .expect("regenerated contour");
        assert!(
            contour_after_revert
                .vertices
                .iter()
                .all(|v| v.location.x < 400.0),
            "contour-only stretch must not survive regeneration: {:?}",
            contour_after_revert.vertices
        );

        // --- Correct (fixed) path: `stretch_wall_axis_in_window` — the
        // exact helper `CmdResult::StretchEntities` now calls for wall
        // packages — moves the axis itself, then regenerates.
        let touched_after_fix = stretch_wall_axis_in_window(
            &mut scene, 
            wall_handle, 
            |x,  _y| x < 400.0, 
            DVec3::new(495.0, 0.0, 0.0), 
            None, 
            None,
            None)
        .expect("axis vertex fell inside the window; must return Some(touched)");
        let axis_moved = get_wall_vertices(&scene, wall_handle)
            .iter()
            .any(|v| v.x > 400.0);
        assert!(
            axis_moved,
            "moving the axis vertices must stick after regeneration"
        );
        let contour_moved = touched_after_fix
            .iter()
            .filter_map(|&h| match scene.document.get_entity(h) {
                Some(EntityType::LwPolyline(pl)) if pl.is_closed => Some(pl.clone()),
                _ => None,
            })
            .any(|pl| pl.vertices.iter().any(|v| v.location.x > 400.0));
        assert!(
            contour_moved,
            "the regenerated contour must reach the new axis extent after the fix"
        );

        // A window that covers none of the axis vertices must be a no-op.
        assert!(
            stretch_wall_axis_in_window(
                &mut scene, 
                wall_handle, 
                |_x,  _y| false, 
                DVec3::new(1.0, 0.0, 0.0), 
                None, 
                None,
                None)
            .is_none(),
            "a window matching no axis vertex must not move or regenerate the wall"
        );
    }

    #[test]
    fn join_l_corner_layer_footprints_share_miter_boundary() {
        let mut scene = Scene::new();
        // Single-layer walls so both sides fully match for miter.
        let mut pl_a = LwPolyline::new();
        pl_a.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut ent_a = EntityType::LwPolyline(pl_a);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
        rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_a.common_mut().extended_data.add_record(rec_a);
        let wall_a = scene.add_entity(ent_a);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut ent_b = EntityType::LwPolyline(pl_b);
        let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
        rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_b.common_mut().extended_data.add_record(rec_b);
        let wall_b = scene.add_entity(ent_b);

        let (kind, touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("L join");
        assert_eq!(kind, JoinKind::L);
        assert!(touched.contains(&wall_a) && touched.contains(&wall_b));

        // Persisted axes still meet exactly at the corner.
        let axis_a = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert_eq!(*axis_a.last().unwrap(), DVec3::new(6.0, 0.0, 0.0));
        assert_eq!(*axis_b.first().unwrap(), DVec3::new(6.0, 0.0, 0.0));

        // Collect closed contour polylines (layer footprints) for both walls.
        let contours = |scene: &Scene, h: Handle| -> Vec<Vec<(f64, f64)>> {
            let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
            wall.derived_handles
                .iter()
                .filter_map(|dh| match scene.document.get_entity(*dh) {
                    Some(EntityType::LwPolyline(pl)) if pl.is_closed => Some(
                        pl.vertices
                            .iter()
                            .map(|v| (v.location.x, v.location.y))
                            .collect(),
                    ),
                    _ => None,
                })
                .collect()
        };
        let fps_a = contours(&scene, wall_a);
        let fps_b = contours(&scene, wall_b);
        assert!(!fps_a.is_empty() && !fps_b.is_empty());

        // Expected shared miter corners for equal 0.2 walls at (6,0):
        // (6.1, -0.1) and (5.9, 0.1).
        let c1 = (6.1, -0.1);
        let c2 = (5.9, 0.1);
        let has = |fp: &[(f64, f64)], p: (f64, f64)| {
            fp.iter()
                .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
        };
        assert!(
            fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "wall A footprint should contain both miter corners, got {fps_a:?}"
        );
        assert!(
            fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "wall B footprint should share the same miter corners (no gap), got {fps_b:?}"
        );
    }

    #[test]
    fn join_t_corner_stem_footprint_reaches_through_wall_face() {
        let mut scene = Scene::new();
        // Stem A approaching through wall B from above.
        let mut pl_a = LwPolyline::new();
        pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
        pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 10.0)));
        let mut ent_a = EntityType::LwPolyline(pl_a);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
        rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_a.common_mut().extended_data.add_record(rec_a);
        let wall_a = scene.add_entity(ent_a);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut ent_b = EntityType::LwPolyline(pl_b);
        let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
        rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_b.common_mut().extended_data.add_record(rec_b);
        let wall_b = scene.add_entity(ent_b);

        let (kind, _touched) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("T join");
        assert_eq!(kind, JoinKind::T);

        let axis_a = get_wall_vertices(&scene, wall_a);
        assert!(
            axis_a
                .iter()
                .any(|p| (p.x - 5.0).abs() < 1e-6 && p.y.abs() < 1e-6),
            "stem axis should end on the through wall axis, got {axis_a:?}"
        );

        let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
        let mut max_abs_y_near_join = 0.0_f64;
        for h in &wall_a_v2.derived_handles {
            if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
                for v in &pl.vertices {
                    if (v.location.x - 5.0).abs() < 0.15 {
                        max_abs_y_near_join = max_abs_y_near_join.max(v.location.y.abs());
                    }
                }
            }
        }
        assert!(
            max_abs_y_near_join > 0.05,
            "stem footprint should reach the through wall's layer face, got max |y|={max_abs_y_near_join}"
        );
    }

    #[test]
    fn join_t_does_not_shorten_through_axis() {
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let stem = add(&mut scene, (4.0, 1.0), (4.0, 8.0));
        let (kind, _) = join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");
        assert_eq!(kind, JoinKind::T);
        let through_axis = get_wall_vertices(&scene, through);
        assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
        let stem_axis = get_wall_vertices(&scene, stem);
        assert!(
            stem_axis[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 1e-6,
            "stem should end on the through axis, got {stem_axis:?}"
        );
    }

    #[test]
    fn join_junction_resolves_two_wall_l_and_t() {
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
        let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
        let touched = join_junction_in_document(&mut scene,  &[a, b],  None,  None,  None,  None).expect("L junction");
        assert!(touched.contains(&a) && touched.contains(&b));
        let axis_a = get_wall_vertices(&scene, a);
        let axis_b = get_wall_vertices(&scene, b);
        assert_eq!(*axis_a.last().unwrap(), DVec3::new(5.0, 0.0, 0.0));
        assert_eq!(*axis_b.first().unwrap(), DVec3::new(5.0, 0.0, 0.0));

        let through = add(&mut scene, (0.0, 10.0), (10.0, 10.0));
        let stem = add(&mut scene, (3.0, 10.0), (3.0, 15.0));
        join_junction_in_document(&mut scene,  &[through, stem],  None,  None,  None,  None).expect("T junction");
        let through_axis = get_wall_vertices(&scene, through);
        assert_eq!(through_axis[0], DVec3::new(0.0, 10.0, 0.0));
        assert_eq!(through_axis[1], DVec3::new(10.0, 10.0, 0.0));
    }

    #[test]
    fn join_junction_honors_display_rules_on_both_walls() {
        use engine::display_component::{ComponentRuleSet, WallComponentSlot};
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
        let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Solid3D.key().to_string(), false);
        join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
            .expect("L junction");
        for h in [a, b] {
            let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
                .expect("WALL")
                .derived_handles;
            let solids = derived
                .iter()
                .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
                .count();
            assert_eq!(
                solids, 0,
                "join regen must honor the active plan (no 3D solids), wall {h:?}"
            );
            assert!(
                !derived.is_empty(),
                "2D representation should still be created"
            );
        }
    }

    #[test]
    fn refresh_after_axis_edit_honors_display_rules_on_joined_walls() {
        use engine::display_component::{ComponentRuleSet, WallComponentSlot};
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let a = add(&mut scene, (0.0, 0.0), (5.0, 0.0));
        let b = add(&mut scene, (5.0, 0.0), (5.0, 5.0));
        let mut rules = ComponentRuleSet::default();
        rules
            .visibility
            .insert(WallComponentSlot::Solid3D.key().to_string(), false);
        join_junction_in_document(&mut scene, &[a, b], None, None, Some(&rules), None)
            .expect("L junction");
        let mut axis = get_wall_vertices(&scene, a);
        axis[0] = DVec3::new(-0.2, 0.0, 0.0);
        update_wall_vertices(&mut scene, a, &axis);
        refresh_wall_after_axis_edit(&mut scene, a, None, Some(&rules), None);
        for h in [a, b] {
            let derived = wall_from_entity(scene.document.get_entity(h).unwrap())
                .expect("WALL")
                .derived_handles;
            let solids = derived
                .iter()
                .filter(|handle| matches!(scene.document.get_entity(**handle), Some(EntityType::Solid3D(_))))
                .count();
            assert_eq!(
                solids, 0,
                "axis-edit regen must honor the active plan (no 3D solids), wall {h:?}"
            );
        }
    }

    #[test]
    fn try_auto_join_after_grip_keeps_t_through_axis() {
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let through = add(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let stem = add(&mut scene, (4.0, 0.0), (4.0, 6.0));
        join_two_walls_in_document(&mut scene,  stem,  through,  None,  None,  None).expect("T");

        let mut stem_axis = get_wall_vertices(&scene, stem);
        stem_axis[0] = DVec3::new(4.05, 0.1, 0.0);
        update_wall_vertices(&mut scene, stem, &stem_axis);
        let _ = try_auto_join_nearby_walls(&mut scene,  stem,  None,  None,  None);

        let through_axis = get_wall_vertices(&scene, through);
        assert_eq!(through_axis[0], DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(through_axis[1], DVec3::new(10.0, 0.0, 0.0));
        let stem_after = get_wall_vertices(&scene, stem);
        assert!(
            stem_after[0].distance(DVec3::new(4.05, 0.0, 0.0)) < 1e-6
                || stem_after[0].distance(DVec3::new(4.0, 0.0, 0.0)) < 0.2,
            "stem should re-join the through axis, got {stem_after:?}"
        );
    }

    #[test]
    fn aec_wallextend_to_target_wall_matches_join_wall_axes_intersection() {
        use crate::ui::command_line::CommandLine;

        let mut scene = Scene::new();
        // Wall A: (0,0)->(5,0); wall B vertical at x=8.
        let wall_a = add_multi_layer_wall(&mut scene);
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, -5.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(8.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record(
            "style1",
            3.0,
            0,
            &vec![wl("Concrete", 0.2, "Structural")],
            &[],
            WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        let axis_a_before = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        let (expected_a, _, _, _, _) =
            join::join_wall_axes(&axis_a_before, &axis_b).expect("axes should intersect");

        // Simulate the interactive path: pick source wall, then pick target
        // wall (auto-detect, no W keystroke).
        let mut cmd = WallExtendCommand::new();
        assert!(matches!(
            cmd.on_entity_pick(wall_a, DVec3::new(2.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let dispatch = match cmd.on_entity_pick(wall_b, DVec3::new(8.0, 0.0, 0.0)) {
            CmdResult::Dispatch(s) => s,
            _ => panic!("expected WALL| dispatch"),
        };
        let args = dispatch
            .strip_prefix("AEC_WALLEXTEND_DO ")
            .expect("dispatch prefix");
        let mut command_line = CommandLine::default();
        aec_wallextend_do(&mut scene,  &mut command_line,  args,  None,  None,  None);

        let axis_a_after = get_wall_vertices(&scene, wall_a);
        assert_eq!(
            axis_a_after.len(),
            expected_a.len(),
            "axis vertex count should match join_wall_axes"
        );
        for (got, exp) in axis_a_after.iter().zip(expected_a.iter()) {
            assert!(
                got.distance(*exp) < 1e-9,
                "extended endpoint must match join_wall_axes intersection, got {got:?} expected {exp:?}"
            );
        }
        // Must NOT be the raw click point projected onto the wall direction
        // in a way that ignores B — the intersection is at x=8.
        assert!(axis_a_after
            .iter()
            .any(|p| (p.x - 8.0).abs() < 1e-9 && p.y.abs() < 1e-9));
        // Full join rebuilds both walls' representations (miter/T stem). For
        // this T configuration the through-wall axis vertices stay put, but
        // the stem must still land on x=8 and both walls keep derived handles.
        let wall_a_v2 = wall_from_entity(scene.document.get_entity(wall_a).unwrap()).unwrap();
        let wall_b_v2 = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
        assert!(
            !wall_a_v2.derived_handles.is_empty() && !wall_b_v2.derived_handles.is_empty(),
            "both walls should have regenerated derived handles after extend-join"
        );
    }

    #[test]
    fn find_wall_to_auto_join_picks_nearby_joinable_wall() {
        let mut scene = Scene::new();
        // Existing wall along X from (0,0) to (5,0).
        let existing = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

        // New wall ending 0.15 m short of existing's end — within snap radius.
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.15, 0.15)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let new_wall = scene.add_entity(entity);

        let found = find_wall_to_auto_join(&scene, new_wall, &[]);
        assert_eq!(
            found,
            Some(existing),
            "should find the nearby existing wall within WALL_JOIN_SNAP_RADIUS"
        );

        // Far wall: no candidate.
        let mut pl_far = LwPolyline::new();
        pl_far.add_vertex(LwVertex::new(Vector2::new(50.0, 0.0)));
        pl_far.add_vertex(LwVertex::new(Vector2::new(55.0, 0.0)));
        let mut ent_far = EntityType::LwPolyline(pl_far);
        let mut rec_far = ExtendedDataRecord::new(AEC_APPID);
        rec_far.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_far.common_mut().extended_data.add_record(rec_far);
        let far = scene.add_entity(ent_far);
        assert!(
            find_wall_to_auto_join(&scene, far, &[]).is_none(),
            "far wall must not auto-join"
        );

        // Excluding the only candidate yields None.
        assert!(find_wall_to_auto_join(&scene, new_wall, &[existing]).is_none());
    }

    #[test]
    fn find_wall_to_auto_join_prefers_clear_endpoint_match_over_closer_t_match() {
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];

        // Candidate L: horizontal wall (0,0)->(5,0), same as
        // `add_multi_layer_wall`. Its endpoint at (5,0) is ~0.18 away from
        // the new wall's lower endpoint below — a clear End-End match.
        let corner = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, corner, None).expect("regen corner");

        // Candidate T: a long horizontal wall running underneath, whose
        // *interior* (not an endpoint) is only ~0.1 away from the new wall's
        // lower endpoint — nominally closer, but a vaguer End-Mid match.
        let mut pl_through = LwPolyline::new();
        pl_through.add_vertex(LwVertex::new(Vector2::new(-5.0, 0.2)));
        pl_through.add_vertex(LwVertex::new(Vector2::new(15.0, 0.2)));
        let mut ent_through = EntityType::LwPolyline(pl_through);
        let mut rec_through = ExtendedDataRecord::new(AEC_APPID);
        rec_through.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_through.common_mut().extended_data.add_record(rec_through);
        let through = scene.add_entity(ent_through);
        regenerate_wall_representation(&mut scene, through, None).expect("regen through");

        // New wall: vertical, ending at (5.15, 0.1) — closer in raw distance
        // to `through`'s interior (~0.1) than to `corner`'s endpoint (~0.18).
        let mut pl_new = LwPolyline::new();
        pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 3.0)));
        pl_new.add_vertex(LwVertex::new(Vector2::new(5.15, 0.1)));
        let mut entity = EntityType::LwPolyline(pl_new);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let new_wall = scene.add_entity(entity);

        let found = find_wall_to_auto_join(&scene, new_wall, &[]);
        assert_eq!(
            found,
            Some(corner),
            "a clear endpoint match should win over a nominally closer T-interior match"
        );
    }

    #[test]
    fn find_wall_to_auto_join_never_returns_the_wall_itself() {
        let mut scene = Scene::new();
        let existing = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");
        // Even without excluding it explicitly, the candidate loop skips
        // `wall_handle == other` unconditionally, so a wall can never
        // auto-join to itself while still being drawn/edited.
        assert_eq!(find_wall_to_auto_join(&scene, existing, &[]), None);
    }

    #[test]
    fn try_auto_join_nearby_walls_joins_axes_and_returns_touched_handles() {
        let mut scene = Scene::new();
        // Existing: (0,0)->(5,0). New wall approaches an L corner near (5,0).
        let existing = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, existing, None).expect("regen existing");

        let mut pl = LwPolyline::new();
        // End slightly short of the true intersection (5,0) — within snap radius.
        pl.add_vertex(LwVertex::new(Vector2::new(5.1, 4.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(5.1, 0.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let new_wall = scene.add_entity(entity);
        regenerate_wall_representation(&mut scene, new_wall, None).expect("regen new");

        let touched = try_auto_join_nearby_walls(&mut scene,  new_wall,  None,  None,  None);
        assert!(
            !touched.is_empty(),
            "auto-join should touch both walls' packages"
        );
        assert!(
            touched.contains(&new_wall) && touched.contains(&existing),
            "touched set must include both wall axes, got {touched:?}"
        );

        // Axes must meet at the true intersection.
        let axis_new = get_wall_vertices(&scene, new_wall);
        let axis_ex = get_wall_vertices(&scene, existing);
        let meet = DVec3::new(5.1, 0.0, 0.0); // vertical at x=5.1 meets horizontal y=0
        // join_wall_axes extends the horizontal wall end and moves the vertical end.
        assert!(
            axis_new
                .iter()
                .any(|p| p.distance(meet) < 1e-6)
                || axis_ex.iter().any(|p| {
                    axis_new.iter().any(|q| p.distance(*q) < 1e-6)
                }),
            "after auto-join the walls should share an axis intersection; new={axis_new:?} existing={axis_ex:?}"
        );

        // Both walls should still have derived representation handles.
        for h in [new_wall, existing] {
            let v2 = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
            assert!(
                !v2.derived_handles.is_empty(),
                "wall {} should retain derived handles after auto-join",
                h.value()
            );
            for d in &v2.derived_handles {
                assert!(
                    touched.contains(d),
                    "derived handle {} must be in touched set",
                    d.value()
                );
            }
        }

        let peers_new = engine::owner_index::peers_of(&scene.document, new_wall);
        let peers_ex = engine::owner_index::peers_of(&scene.document, existing);
        assert!(
            peers_new.contains(&existing),
            "auto-join must record JOINED_PEERS on the new wall, got {peers_new:?}"
        );
        assert!(
            peers_ex.contains(&new_wall),
            "auto-join must record JOINED_PEERS on the existing wall, got {peers_ex:?}"
        );
    }

    #[test]
    fn try_auto_join_t_records_joined_peers() {
        let mut scene = Scene::new();
        let through = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        regenerate_wall_representation(&mut scene, through, None).expect("regen through");
        let stem = add_single_layer_wall(&mut scene, (5.0, 0.05), (5.0, 4.0));
        regenerate_wall_representation(&mut scene, stem, None).expect("regen stem");
        let _ = try_auto_join_nearby_walls(&mut scene, stem, None, None, None);
        let peers = engine::owner_index::peers_of(&scene.document, stem);
        assert!(
            peers.contains(&through),
            "T auto-join must list the through wall as JOINED_PEERS, got {peers:?}"
        );
        let peers_t = engine::owner_index::peers_of(&scene.document, through);
        assert!(
            peers_t.contains(&stem),
            "T auto-join must list the stem on the through wall, got {peers_t:?}"
        );
    }

    #[test]
    fn apply_junction_override_rebuilds_partner_wall() {
        let mut scene = Scene::new();
        let wall_a = add_multi_layer_wall(&mut scene);
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(6.0, 10.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.3, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values = wall_record(
            "style1",
            3.0,
            0,
            &layers_b,
            &[],
            WallJustification::Center,
            PlanPhase::New,
            None,
        );
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);
        regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");
        join_two_walls_in_document(&mut scene, wall_a, wall_b, None, None, None)
            .expect("join");

        let ov = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::NoExtend),
            layer_pairs: Vec::new(),
        };
        let touched = apply_junction_override_and_rebuild(
            &mut scene,
            wall_a,
            1,
            Some(&ov),
            None,
            None,
            None,
        );
        assert!(touched.contains(&wall_a) && touched.contains(&wall_b));
        assert!(read_junction_override(&scene, wall_a, 1).is_some());
    }

    fn plaster_masonry_plaster() -> Vec<WallLayer> {
        wls(&[
            ("Putz", 0.015, "Finish"),
            ("Mauerwerk", 0.24, "Structural"),
            ("Putz", 0.015, "Finish"),
        ])
    }

    fn plaster_masonry_insulation_plaster() -> Vec<WallLayer> {
        wls(&[
            ("Putz", 0.015, "Finish"),
            ("Mauerwerk", 0.24, "Structural"),
            ("Insulation", 0.12, "Insulation"),
            ("Putz", 0.015, "Finish"),
        ])
    }

    fn join_l_pair(
        scene: &mut Scene,
        a: Handle,
        b: Handle,
    ) {
        let lib = crate::modules::aec::engine::library::seed_default_library();
        let _ = regenerate_wall_representation(scene, a, Some(&lib));
        let _ = regenerate_wall_representation(scene, b, Some(&lib));
        let _ = join_two_walls_in_document(scene, a, b, Some(&lib), None, None);
    }

    fn add_example_note(scene: &mut Scene, x: f64, y: f64, value: &str) {
        use acadrust::entities::MText;
        let mut t = MText::with_value(value.to_string(), Vector3::new(x, y, 0.0));
        t.height = 0.28;
        scene.add_entity(EntityType::MText(t));
    }

    #[test]
    fn write_aec_wall_join_examples_dxf() {
        let mut scene = Scene::new();
        let lib = crate::modules::aec::engine::library::seed_default_library();

        // L: einschalig
        let l_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
        let l_b = add_single_layer_wall(&mut scene, (5.0, 0.0), (5.0, 5.0));
        join_l_pair(&mut scene, l_a, l_b);
        add_example_note(
            &mut scene,
            0.0,
            -1.4,
            "L einschalig\\PStil: style1 (Concrete)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
        );

        // L: Putz + Mauerwerk + Putz
        let l3_a = add_layered_wall(&mut scene, (10.0, 0.0), (16.0, 0.0), plaster_masonry_plaster());
        let l3_b = add_layered_wall(&mut scene, (16.0, 0.0), (16.0, 5.0), plaster_masonry_plaster());
        join_l_pair(&mut scene, l3_a, l3_b);
        add_example_note(
            &mut scene,
            10.0,
            -1.4,
            "L dreischalig\\PStil: style1 (Putz+Mauerwerk+Putz)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
        );

        // L: Putz + Mauerwerk + Dämmung + Putz
        let l4_a = add_layered_wall(
            &mut scene,
            (20.0, 0.0),
            (26.0, 0.0),
            plaster_masonry_insulation_plaster(),
        );
        let l4_b = add_layered_wall(
            &mut scene,
            (26.0, 0.0),
            (26.0, 5.0),
            plaster_masonry_insulation_plaster(),
        );
        join_l_pair(&mut scene, l4_a, l4_b);
        add_example_note(
            &mut scene,
            20.0,
            -1.4,
            "L vierschalig\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: L, Standard-Miter\\POverride: keiner",
        );

        // T: gleiche 4-Schalen (Stamm trifft Durchgang)
        let t4_through = add_layered_wall(
            &mut scene,
            (0.0, 10.0),
            (10.0, 10.0),
            plaster_masonry_insulation_plaster(),
        );
        let t4_stem = add_layered_wall(
            &mut scene,
            (5.0, 10.05),
            (5.0, 16.0),
            plaster_masonry_insulation_plaster(),
        );
        let _ = regenerate_wall_representation(&mut scene, t4_through, Some(&lib));
        let _ = regenerate_wall_representation(&mut scene, t4_stem, Some(&lib));
        let _ = try_auto_join_nearby_walls(&mut scene, t4_stem, Some(&lib), None, None);
        add_example_note(
            &mut scene,
            0.0,
            8.3,
            "T vierschalig\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: T (Auto-Join)\\PStamm → Durchgang, Standard-Miter\\POverride: keiner",
        );

        // T: gleiche 3-Schalen
        let t3_through = add_layered_wall(
            &mut scene,
            (14.0, 10.0),
            (24.0, 10.0),
            plaster_masonry_plaster(),
        );
        let t3_stem = add_layered_wall(
            &mut scene,
            (19.0, 10.05),
            (19.0, 16.0),
            plaster_masonry_plaster(),
        );
        let _ = regenerate_wall_representation(&mut scene, t3_through, Some(&lib));
        let _ = regenerate_wall_representation(&mut scene, t3_stem, Some(&lib));
        let _ = try_auto_join_nearby_walls(&mut scene, t3_stem, Some(&lib), None, None);
        add_example_note(
            &mut scene,
            14.0,
            8.3,
            "T dreischalig\\PStil: style1 (Putz+Mauerwerk+Putz)\\PVerbindung: T (Auto-Join)\\PStamm → Durchgang, Standard-Miter\\POverride: keiner",
        );

        // N-Wege einschalig
        let n1 = add_single_layer_wall(&mut scene, (0.0, 22.0), (8.0, 22.0));
        let n2 = add_single_layer_wall(&mut scene, (8.0, 22.0), (8.0, 28.0));
        let n3 = add_single_layer_wall(&mut scene, (8.0, 22.0), (8.0, 16.0));
        for h in [n1, n2, n3] {
            let _ = regenerate_wall_representation(&mut scene, h, Some(&lib));
        }
        let _ = join_junction_in_document(&mut scene, &[n1, n2, n3], None, Some(&lib), None, None);
        add_example_note(
            &mut scene,
            0.0,
            29.2,
            "N-Wege einschalig\\PStil: style1 (Concrete)\\PVerbindung: N-Wege (3 Wände)\\PStandard-Miter\\POverride: keiner",
        );

        // L 4-schalig mit individuellen Schichtverbindungen:
        // Putz außen/innen NoExtend, Mauerwerk Miter, Dämmung Butt.
        let ov_a = add_layered_wall(
            &mut scene,
            (14.0, 22.0),
            (22.0, 22.0),
            plaster_masonry_insulation_plaster(),
        );
        let ov_b = add_layered_wall(
            &mut scene,
            (22.0, 22.0),
            (22.0, 28.0),
            plaster_masonry_insulation_plaster(),
        );
        join_l_pair(&mut scene, ov_a, ov_b);
        let layers = wall_from_entity(scene.document.get_entity(ov_a).unwrap())
            .unwrap()
            .layers;
        let refs = layer_refs_from_materials(
            layers.iter().map(|l| (l.material.as_str(), l.layer_id)),
        );
        let ov = join::JunctionOverride {
            default_style: Some(join::JoinOverrideStyle::Miter),
            layer_pairs: vec![
                join::LayerPairOverride {
                    layer_a: refs[0].clone(),
                    layer_b: None,
                    style: join::JoinOverrideStyle::NoExtend,
                },
                join::LayerPairOverride {
                    layer_a: refs[1].clone(),
                    layer_b: refs.get(1).cloned(),
                    style: join::JoinOverrideStyle::Miter,
                },
                join::LayerPairOverride {
                    layer_a: refs[2].clone(),
                    layer_b: None,
                    style: join::JoinOverrideStyle::Butt,
                },
                join::LayerPairOverride {
                    layer_a: refs[3].clone(),
                    layer_b: None,
                    style: join::JoinOverrideStyle::NoExtend,
                },
            ],
        };
        let _ = apply_junction_override_and_rebuild(
            &mut scene, ov_a, 1, Some(&ov), Some(&lib), None, None,
        );
        add_example_note(
            &mut scene,
            14.0,
            29.2,
            "L vierschalig mit Schicht-Overrides\\PStil: style1 (Putz+Mauerwerk+Insulation+Putz)\\PVerbindung: L\\PDefault: Miter\\PPutz außen/innen: NoExtend\\PMauerwerk: Miter\\PDämmung: Butt",
        );

        let out = std::path::Path::new("docs/examples/aec-wall-joins.dxf");
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        acadrust::DxfWriter::new(&scene.document)
            .write_to_file(out)
            .expect("write example drawing");
        assert!(out.exists());
    }

    #[test]
    fn try_auto_join_nearby_walls_is_noop_when_nothing_nearby() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        regenerate_wall_representation(&mut scene, wall, None).expect("regen");
        let before = get_wall_vertices(&scene, wall);
        let touched = try_auto_join_nearby_walls(&mut scene,  wall,  None,  None,  None);
        assert!(touched.is_empty(), "no partner → no touched handles");
        let after = get_wall_vertices(&scene, wall);
        assert_eq!(before, after, "axis must be unchanged when auto-join is a no-op");
    }

    /// Collect min/max Y of every derived LwPolyline vertex for a wall package.
    fn wall_derived_y_bounds(scene: &Scene, wall: Handle) -> (f64, f64) {
        let v2 = wall_from_entity(scene.document.get_entity(wall).unwrap()).unwrap();
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for h in &v2.derived_handles {
            if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
                for v in &pl.vertices {
                    ymin = ymin.min(v.location.y);
                    ymax = ymax.max(v.location.y);
                }
            }
        }
        (ymin, ymax)
    }

    #[test]
    fn reverse_wall_preserves_footprint_and_flips_layer_side_assignment() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        // Concrete 0.2 + Insulation 0.05, total 0.25 → y ∈ [-0.125, 0.125]
        regenerate_wall_representation(&mut scene, wall, None).expect("regen");

        let axis_before = get_wall_vertices(&scene, wall);
        let layers_before = wall_from_entity(scene.document.get_entity(wall).unwrap())
            .unwrap()
            .layers
            .clone();
        let (ymin_before, ymax_before) = wall_derived_y_bounds(&scene, wall);

        // Contour of first layer (Concrete) should sit on the more-negative side.
        let entity = scene.document.get_entity(wall).unwrap().clone();
        let contours_before = wall_layer_contour_polylines(&entity, &layers_before);
        let concrete_y_before = contours_before[0]
            .0
            .iter()
            .chain(contours_before[0].1.iter())
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);

        reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

        let axis_after = get_wall_vertices(&scene, wall);
        assert_eq!(
            axis_after,
            axis_before.iter().rev().copied().collect::<Vec<_>>(),
            "axis vertices must reverse order"
        );

        let layers_after = wall_from_entity(scene.document.get_entity(wall).unwrap())
            .unwrap()
            .layers
            .clone();
        assert_eq!(layers_after.len(), layers_before.len());
        // The stored layer list itself is untouched by reverse; the axis
        // direction flip alone is what relocates each material.
        assert_eq!(layers_after[0].material, layers_before[0].material);
        assert_eq!(
            layers_after.last().unwrap().material,
            layers_before.last().unwrap().material
        );

        let (ymin_after, ymax_after) = wall_derived_y_bounds(&scene, wall);
        assert!(
            (ymin_before - ymin_after).abs() < 1e-6 && (ymax_before - ymax_after).abs() < 1e-6,
            "outer footprint Y bounds must stay identical: before=({ymin_before},{ymax_before}) after=({ymin_after},{ymax_after})"
        );

        // Materials must visibly swap sides: Concrete (still layers[0]) now
        // sits on the more-positive side, since the axis direction flip
        // inverted the offset normal used to place it.
        let entity_after = scene.document.get_entity(wall).unwrap().clone();
        let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
        let concrete_y_after = contours_after[0]
            .0
            .iter()
            .chain(contours_after[0].1.iter())
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        assert!(
            (concrete_y_before - concrete_y_after).abs() > 1e-6,
            "Concrete must move to the opposite absolute side after reverse: before={concrete_y_before} after={concrete_y_after}"
        );
    }

    #[test]
    fn reverse_wall_keeps_joined_corner_intersection() {
        let mut scene = Scene::new();
        // Wall A: (0,0)->(5,0). Wall B: (5,0)->(5,5) L-corner.
        let wall_a = add_multi_layer_wall(&mut scene);

        // Approach the L corner from above so join_wall_axes must extend B.
        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let mut entity_b = EntityType::LwPolyline(pl_b);
        let layers_b = vec![wl("Concrete", 0.2, "Structural")];
        let mut record_b = ExtendedDataRecord::new(AEC_APPID);
        record_b.values =
            wall_record("style1", 3.0, 0, &layers_b, &[], WallJustification::Center, PlanPhase::New, None);
        entity_b.common_mut().extended_data.add_record(record_b);
        let wall_b = scene.add_entity(entity_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
        let (_kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");

        let axis_a_before = get_wall_vertices(&scene, wall_a);
        let axis_b_before = get_wall_vertices(&scene, wall_b);
        // Record the shared corner (any vertex of A that coincides with B).
        let corner = axis_a_before
            .iter()
            .find(|pa| axis_b_before.iter().any(|pb| pa.distance(*pb) < 1e-6))
            .copied()
            .expect("joined walls must share a corner before reverse");

        reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

        let axis_a = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        // After reverse + auto-join, the axes must still share an intersection
        // (at the original corner or a re-joined equivalent).
        let a_has = axis_a.iter().any(|p| p.distance(corner) < 1e-4);
        let b_has = axis_b.iter().any(|p| p.distance(corner) < 1e-4);
        let share = axis_a
            .iter()
            .any(|pa| axis_b.iter().any(|pb| pa.distance(*pb) < 1e-4));
        assert!(
            (a_has && b_has) || share,
            "joined corner must remain correct after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
        );
    }

    /// Helper: closed LwPolyline layer footprints for a wall package.
    fn wall_closed_footprints(scene: &Scene, h: Handle) -> Vec<Vec<(f64, f64)>> {
        let wall = wall_from_entity(scene.document.get_entity(h).unwrap()).unwrap();
        wall.derived_handles
            .iter()
            .filter_map(|dh| match scene.document.get_entity(*dh) {
                Some(EntityType::LwPolyline(pl)) if pl.is_closed => Some(
                    pl.vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    /// Reversing a wall that is *already* part of a stable L-join must
    /// re-detect the join (axes already coincident — endpoints don't move)
    /// and rebuild *both* walls' derived geometry with true miter corners,
    /// not plain rectangular end-caps.
    #[test]
    fn reverse_already_joined_wall_rebuilds_mitered_geometry_on_both() {
        let mut scene = Scene::new();
        // Single-layer equal walls so miter matching is unambiguous.
        // A: (0,0)->(5,0). B approaches from above near (5,0).
        let mut pl_a = LwPolyline::new();
        pl_a.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl_a.add_vertex(LwVertex::new(Vector2::new(5.0, 0.0)));
        let mut ent_a = EntityType::LwPolyline(pl_a);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut rec_a = ExtendedDataRecord::new(AEC_APPID);
        rec_a.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_a.common_mut().extended_data.add_record(rec_a);
        let wall_a = scene.add_entity(ent_a);

        let mut pl_b = LwPolyline::new();
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
        pl_b.add_vertex(LwVertex::new(Vector2::new(5.0, 5.0)));
        let mut ent_b = EntityType::LwPolyline(pl_b);
        let mut rec_b = ExtendedDataRecord::new(AEC_APPID);
        rec_b.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent_b.common_mut().extended_data.add_record(rec_b);
        let wall_b = scene.add_entity(ent_b);

        regenerate_wall_representation(&mut scene, wall_a, None).expect("regen A");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("regen B");
        let (kind, _) = join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("join");
        assert_eq!(kind, JoinKind::L);

        let axis_a_joined = get_wall_vertices(&scene, wall_a);
        let axis_b_joined = get_wall_vertices(&scene, wall_b);
        let corner = axis_a_joined
            .iter()
            .find(|pa| axis_b_joined.iter().any(|pb| pa.distance(*pb) < 1e-6))
            .copied()
            .expect("joined walls share a corner");

        // Capture pre-reverse miter corners (equal 0.2 walls at corner (5,0):
        // (5.1, -0.1) and (4.9, 0.1)).
        let fps_a_before = wall_closed_footprints(&scene, wall_a);
        let fps_b_before = wall_closed_footprints(&scene, wall_b);
        let c1 = (corner.x + 0.1, corner.y - 0.1);
        let c2 = (corner.x - 0.1, corner.y + 0.1);
        let has = |fp: &[(f64, f64)], p: (f64, f64)| {
            fp.iter()
                .any(|(x, y)| (*x - p.0).abs() < 1e-6 && (*y - p.1).abs() < 1e-6)
        };
        assert!(
            fps_a_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "precondition: A must be mitered before reverse, got {fps_a_before:?}"
        );
        assert!(
            fps_b_before.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "precondition: B must be mitered before reverse, got {fps_b_before:?}"
        );

        // Reverse the *already-joined* wall A. This is the live-app scenario:
        // join happened earlier; reverse must re-join without relying on
        // endpoints moving.
        let touched = reverse_wall_in_document(&mut scene,  wall_a,  None,  None,  None).expect("reverse A");

        // Both packages must be in the touched set (B's derived geometry is
        // regenerated too, not only A's axis/XDATA).
        assert!(
            touched.contains(&wall_a),
            "touched must include reversed wall A"
        );
        assert!(
            touched.contains(&wall_b),
            "touched must include neighbour B after re-join, got {touched:?}"
        );
        let v2_b = wall_from_entity(scene.document.get_entity(wall_b).unwrap()).unwrap();
        for d in &v2_b.derived_handles {
            assert!(
                touched.contains(d),
                "B derived handle {} must be bumped after reverse+rejoin",
                d.value()
            );
        }

        let axis_a = get_wall_vertices(&scene, wall_a);
        let axis_b = get_wall_vertices(&scene, wall_b);
        assert!(
            axis_a.iter().any(|p| p.distance(corner) < 1e-4)
                && axis_b.iter().any(|p| p.distance(corner) < 1e-4),
            "corner must stay put after reverse; corner={corner:?} A={axis_a:?} B={axis_b:?}"
        );
        // Axis order flipped on A; the join corner vertex is now at the
        // opposite index, but its *position* is unchanged.
        assert!(
            axis_a.first().unwrap().distance(corner) < 1e-4
                || axis_a.last().unwrap().distance(corner) < 1e-4,
            "A's join endpoint still at corner after reverse, A={axis_a:?}"
        );

        let fps_a = wall_closed_footprints(&scene, wall_a);
        let fps_b = wall_closed_footprints(&scene, wall_b);
        assert!(
            fps_a.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "A must keep mitered (non-separator) corner after reverse, got {fps_a:?}"
        );
        assert!(
            fps_b.iter().any(|fp| has(fp, c1) && has(fp, c2)),
            "B must keep mitered corner after neighbour reverse, got {fps_b:?}"
        );
    }

    /// Absolute world-space layer center offsets must mirror (negate) for
    /// an asymmetric 3-layer wall (different thicknesses and gaps) after a
    /// direction reverse — the stored layer list itself is untouched, only
    /// the axis-direction flip relocates each material to the opposite side.
    #[test]
    fn reverse_wall_preserves_three_layer_world_centers() {
        let mut scene = Scene::new();
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut entity = EntityType::LwPolyline(pl);
        // Asymmetric stack: Brick 0.1, gap 0.02, Insulation 0.05, gap 0.01, Concrete 0.2
        let mut brick = wl("Brick", 0.1, "Finish");
        brick.axis_offset = 0.0;
        let mut insulation = wl("Insulation", 0.05, "Insulation");
        insulation.axis_offset = 0.02;
        let mut concrete = wl("Concrete", 0.2, "Structural");
        concrete.axis_offset = 0.01;
        let layers = vec![brick, insulation, concrete];
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        entity.common_mut().extended_data.add_record(record);
        let wall = scene.add_entity(entity);

        regenerate_wall_representation(&mut scene, wall, None).expect("regen");

        let entity_before = scene.document.get_entity(wall).unwrap().clone();
        let layers_before = wall_from_entity(&entity_before).unwrap().layers.clone();
        let contours_before = wall_layer_contour_polylines(&entity_before, &layers_before);

        // Per-material world center offset along the axis normal (Y for a
        // horizontal wall at Y=0): mean of the two boundary Y values.
        let centers_before: Vec<(String, f64)> = layers_before
            .iter()
            .zip(contours_before.iter())
            .map(|(layer, (b1, b2))| {
                let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
                let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
                (layer.material.clone(), 0.5 * (y1 + y2))
            })
            .collect();

        reverse_wall_in_document(&mut scene,  wall,  None,  None,  None).expect("reverse");

        let entity_after = scene.document.get_entity(wall).unwrap().clone();
        let layers_after = wall_from_entity(&entity_after).unwrap().layers.clone();
        let contours_after = wall_layer_contour_polylines(&entity_after, &layers_after);
        assert_eq!(layers_after.len(), 3);
        // The stored layer list order is untouched by reverse.
        assert_eq!(layers_after[0].material, "Brick");
        assert_eq!(layers_after[1].material, "Insulation");
        assert_eq!(layers_after[2].material, "Concrete");

        let centers_after: std::collections::HashMap<String, f64> = layers_after
            .iter()
            .zip(contours_after.iter())
            .map(|(layer, (b1, b2))| {
                let y1 = b1.iter().map(|(_, y)| *y).sum::<f64>() / b1.len() as f64;
                let y2 = b2.iter().map(|(_, y)| *y).sum::<f64>() / b2.len() as f64;
                (layer.material.clone(), 0.5 * (y1 + y2))
            })
            .collect();

        for (mat, c_before) in &centers_before {
            let c_after = centers_after
                .get(mat)
                .unwrap_or_else(|| panic!("material {mat} missing after reverse"));
            // The axis-direction flip inverts the offset normal, so every
            // material's world-space center must mirror (negate) around the
            // axis line rather than stay put.
            assert!(
                (c_before + c_after).abs() < 1e-9,
                "world center of {mat} must mirror after reverse: before={c_before} after={c_after}"
            );
        }
    }

    #[test]
    fn wall_join_hover_highlight_during_both_picks() {
        let cmd = WallJoinCommand::new();
        assert!(
            cmd.entity_pick_highlights_hover(),
            "highlight during first-wall pick"
        );
        let mut cmd = WallJoinCommand::new();
        let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
        assert!(
            cmd.entity_pick_highlights_hover(),
            "highlight while awaiting second wall"
        );
    }

    #[test]
    fn wall_extend_hover_highlight_during_both_picks() {
        let cmd = WallExtendCommand::new();
        assert!(
            cmd.entity_pick_highlights_hover(),
            "highlight during source-wall pick"
        );
        let mut cmd = WallExtendCommand::new();
        let _ = cmd.on_entity_pick(Handle::new(1), DVec3::ZERO);
        assert!(
            cmd.entity_pick_highlights_hover(),
            "highlight while awaiting target"
        );
    }

    #[test]
    fn formula_layer_resolution_feeds_identical_geometry_for_fixed_styles() {
        // Fixed-only style must produce the same contour geometry through the
        // formula-aware resolver as through a hand-built WallLayer stack.
        let lib = engine::library::seed_default_library();
        let style_id = "style_insulated_ext";
        let resolved = resolve_wall_style_layers(&lib, style_id, None).expect("resolve");
        assert_eq!(resolved.len(), 4);
        assert!((resolved[0].thickness - 0.015).abs() < 1e-12);
        assert!((resolved[1].thickness - 0.175).abs() < 1e-12);
        assert!((resolved[2].thickness - 0.14).abs() < 1e-12);
        assert!((resolved[3].thickness - 0.015).abs() < 1e-12);

        let centerline = vec![(0.0, 0.0), (5.0, 0.0)];
        let layer_data: Vec<(f64, f64)> = resolved
            .iter()
            .map(|l| (l.thickness, l.axis_offset))
            .collect();
        let contours = engine::contour::layer_contours(&centerline, &layer_data);
        assert_eq!(contours.len(), 4);

        // Hand-built equivalent with the same centered axis offsets as the seed style.
        let manual = vec![
            (0.015, -0.1725),
            (0.175, -0.1575),
            (0.14, 0.0175),
            (0.015, 0.1575),
        ];
        let manual_contours = engine::contour::layer_contours(&centerline, &manual);
        assert_eq!(contours.len(), manual_contours.len());
        for (a, b) in contours.iter().zip(manual_contours.iter()) {
            assert_eq!(a.0.len(), b.0.len());
            for (p1, p2) in a.0.iter().zip(b.0.iter()) {
                assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
            }
            for (p1, p2) in a.1.iter().zip(b.1.iter()) {
                assert!((p1.0 - p2.0).abs() < 1e-9 && (p1.1 - p2.1).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn bb_formula_style_changes_resolved_thickness_with_base_width() {
        let mut lib = StyleLibrary::empty();
        lib.materials.push(Material::new(
            "mat_a".into(),
            "A".into(),
            "SOLID".into(),
            0xFFFFFF,
            "Continuous".into(),
        ));
        lib.upsert_wall_style(WallStyle {
            style: Style {
                id: "style_bb".into(),
                name: "BB half".into(),
                object_kind: "Wall".into(),
                parent_style_id: None,
            },
            layers: vec![
                Layer {
                    material_id: "mat_a".into(),
                    thickness: LayerValue::Fixed(0.1),
                    function: LayerFunction::Structural,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                },
                Layer {
                    material_id: "mat_a".into(),
                    thickness: LayerValue::Formula("BB * 0.5".into()),
                    function: LayerFunction::Insulation,
                    axis_offset: LayerValue::Fixed(0.0),
                    bottom_offset: 0.0,
                    top_offset: 0.0,
                    layer_override: None,
                    hatch_override: None,
                    role_tag: None,
                layer_id: uuid::Uuid::new_v4(),
                },
            ],
        display_profiles: std::collections::HashMap::new(),
        });

        let r1 = resolve_wall_style_layers(&lib, "style_bb", Some(0.4)).unwrap();
        let r2 = resolve_wall_style_layers(&lib, "style_bb", Some(0.8)).unwrap();
        assert!((r1[1].thickness - 0.2).abs() < 1e-12);
        assert!((r2[1].thickness - 0.4).abs() < 1e-12);

        let centerline = vec![(0.0, 0.0), (3.0, 0.0)];
        let c1 = engine::contour::layer_contours(
            &centerline,
            &r1.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
        );
        let c2 = engine::contour::layer_contours(
            &centerline,
            &r2.iter().map(|l| (l.thickness, l.axis_offset)).collect::<Vec<_>>(),
        );
        // Different BB must produce different outer extents.
        let y_max = |cs: &[(Vec<(f64, f64)>, Vec<(f64, f64)>)]| {
            cs.iter()
                .flat_map(|(a, b)| a.iter().chain(b.iter()))
                .map(|(_, y)| y.abs())
                .fold(0.0_f64, f64::max)
        };
        assert!(
            (y_max(&c1) - y_max(&c2)).abs() > 1e-6,
            "formula BB must affect geometry extents"
        );
    }

    #[test]
    fn invalid_formula_style_falls_back_without_panic() {
        let mut lib = StyleLibrary::empty();
        lib.materials.push(Material::new(
            "mat_a".into(),
            "A".into(),
            "SOLID".into(),
            0xFFFFFF,
            "Continuous".into(),
        ));
        lib.upsert_wall_style(WallStyle {
            style: Style {
                id: "style_bad".into(),
                name: "Bad formula".into(),
                object_kind: "Wall".into(),
                parent_style_id: None,
            },
            layers: vec![Layer {
                material_id: "mat_a".into(),
                thickness: LayerValue::Formula("NOT_A_VAR / 0".into()),
                function: LayerFunction::Structural,
                axis_offset: LayerValue::Fixed(0.0),
                bottom_offset: 0.0,
                top_offset: 0.0,
                layer_override: None,
                hatch_override: None,
                role_tag: None,
            layer_id: uuid::Uuid::new_v4(),
            }],
        display_profiles: std::collections::HashMap::new(),
        });

        let resolved = resolve_wall_style_layers(&lib, "style_bad", Some(0.3)).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].thickness, 0.0);
    }

    #[test]
    fn join_and_delete_maintain_symmetric_peer_links() {
        let mut scene = Scene::new();
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let add_wall = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(a.0, a.1)));
            pl.add_vertex(LwVertex::new(Vector2::new(b.0, b.1)));
            let mut entity = EntityType::LwPolyline(pl);
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("s", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };
        let ha = add_wall(&mut scene, (0.0, 0.0), (5.0, 0.0));
        let hb = add_wall(&mut scene, (0.0, 0.0), (0.0, 5.0));
        join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("L join");
        assert_eq!(engine::owner_index::peers_of(&scene.document, ha), vec![hb]);
        assert_eq!(engine::owner_index::peers_of(&scene.document, hb), vec![ha]);

        // Snapshot peer XDATA then unlink (simulates undo of join / disconnect).
        let peers_before = engine::owner_index::peers_of(&scene.document, ha);
        assert_eq!(peers_before, vec![hb]);
        engine::owner_index::unlink_peers(&mut scene.document, ha, hb);
        assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
        assert!(engine::owner_index::peers_of(&scene.document, hb).is_empty());
        // re-join
        join_two_walls_in_document(&mut scene,  ha,  hb,  None,  None,  None).expect("rejoin");

        let hc = add_wall(&mut scene, (2.5, -3.0), (2.5, 0.0));
        join_two_walls_in_document(&mut scene,  ha,  hc,  None,  None,  None).expect("T join");
        let mut peers_a = engine::owner_index::peers_of(&scene.document, ha);
        peers_a.sort_by_key(|h| h.value());
        let mut expected = vec![hb, hc];
        expected.sort_by_key(|h| h.value());
        assert_eq!(peers_a, expected);

        // Deleting ha clears it from peers.
        unlink_all_wall_peers(&mut scene, ha);
        assert!(engine::owner_index::peers_of(&scene.document, ha).is_empty());
        assert!(!engine::owner_index::peers_of(&scene.document, hb).contains(&ha));
        assert!(!engine::owner_index::peers_of(&scene.document, hc).contains(&ha));
    }

    #[test]
    fn storey_membership_tracks_add_remove_and_reassign() {
        let mut scene = Scene::new();
        let s0 = ensure_storey_entity(&mut scene, 0, Some(&Storey::new("L0", 0.0, 3.0)));
        let s1 = ensure_storey_entity(&mut scene, 1, Some(&Storey::new("L1", 3.0, 3.0)));
        assert!(walls_for_storey(&scene, s0).is_empty());
        assert!(walls_for_storey(&scene, s1).is_empty());

        let w1 = add_multi_layer_wall(&mut scene);
        let w2 = {
            // second wall, same geometry template
            let mut pl = LwPolyline::new();
            pl.add_vertex(LwVertex::new(Vector2::new(0.0, 1.0)));
            pl.add_vertex(LwVertex::new(Vector2::new(5.0, 1.0)));
            let mut entity = EntityType::LwPolyline(pl);
            let layers = vec![wl("Concrete", 0.2, "Structural")];
            let mut record = ExtendedDataRecord::new(AEC_APPID);
            record.values = wall_record("style1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
            entity.common_mut().extended_data.add_record(record);
            scene.add_entity(entity)
        };

        register_wall_in_storey(&mut scene, w1);
        register_wall_in_storey(&mut scene, w2);
        let mut members0 = walls_for_storey(&scene, s0);
        members0.sort_by_key(|h| h.value());
        let mut expected = vec![w1, w2];
        expected.sort_by_key(|h| h.value());
        assert_eq!(members0, expected);
        assert!(walls_for_storey(&scene, s1).is_empty());

        assert!(set_wall_storey(&mut scene, w1, 1));
        assert_eq!(walls_for_storey(&scene, s0), vec![w2]);
        assert_eq!(walls_for_storey(&scene, s1), vec![w1]);
        let wall = wall_from_entity(scene.document.get_entity(w1).unwrap()).unwrap();
        assert_eq!(wall.storey_id, 1);

        unregister_wall_from_storey(&mut scene, w2);
        assert!(walls_for_storey(&scene, s0).is_empty());
        assert_eq!(walls_for_storey(&scene, s1), vec![w1]);

        // erase-path helper clears remaining membership
        unregister_walls_from_storeys(&mut scene, &[w1]);
        assert!(walls_for_storey(&scene, s1).is_empty());
    }

    #[test]
    fn placing_and_removing_opening_keeps_host_child_handles() {
        let mut scene = Scene::new();
        let wall = add_multi_layer_wall(&mut scene);
        assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
        assert!(openings_for_host_wall(&scene, wall).is_empty());

        let (o1, _) = place_wall_opening(
            &mut scene, 
            wall, 
            DVec3::new(1.5, 0.0, 0.0), 
            engine::openings::OpeningKind::Window, 
        None,  None,  None)
        .expect("place window");
        let (o2, _) = place_wall_opening(
            &mut scene, 
            wall, 
            DVec3::new(3.5, 0.0, 0.0), 
            engine::openings::OpeningKind::Door, 
        None,  None,  None)
        .expect("place door");

        let children = engine::owner_index::children_of(&scene.document, wall);
        assert_eq!(children, vec![o1, o2]);

        let openings = openings_for_host_wall(&scene, wall);
        assert_eq!(openings.len(), 2);
        assert!(openings.iter().any(|o| o.handle == o1 && o.kind == engine::openings::OpeningKind::Window));
        assert!(openings.iter().any(|o| o.handle == o2 && o.kind == engine::openings::OpeningKind::Door));
        // host_wall XDATA still written on the opening entity
        let o1_ent = scene.document.get_entity(o1).unwrap();
        let parsed = opening_from_entity(o1_ent, o1).unwrap();
        assert_eq!(parsed.host_wall, wall);

        remove_wall_opening(&mut scene, o1, None).expect("remove window");
        assert_eq!(
            engine::owner_index::children_of(&scene.document, wall),
            vec![o2]
        );
        let remaining = openings_for_host_wall(&scene, wall);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].handle, o2);
        assert!(scene.document.get_entity(o1).is_none());

        remove_wall_opening(&mut scene, o2, None).expect("remove door");
        assert!(engine::owner_index::children_of(&scene.document, wall).is_empty());
        assert!(openings_for_host_wall(&scene, wall).is_empty());
    }

    #[test]
    fn two_junctions_on_one_wall_are_handled_correctly() {
        let mut scene = Scene::new();
        // Wall 1: horizontal along Y=0 from X=0 to X=10.
        let mut pl1 = LwPolyline::new();
        pl1.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl1.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        let mut ent1 = EntityType::LwPolyline(pl1);
        let layers = vec![wl("Concrete", 0.2, "Structural")];
        let mut rec1 = ExtendedDataRecord::new(AEC_APPID);
        rec1.values = wall_record("s1", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent1.common_mut().extended_data.add_record(rec1);
        let w1 = scene.add_entity(ent1);
        regenerate_wall_representation(&mut scene, w1, None).expect("regen w1");

        // Junction A at (0,0): w1 + w2 + w3
        // w2: vertical from (0,0) to (0,5)
        let mut pl2 = LwPolyline::new();
        pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl2.add_vertex(LwVertex::new(Vector2::new(0.0, 5.0)));
        let mut ent2 = EntityType::LwPolyline(pl2);
        let mut rec2 = ExtendedDataRecord::new(AEC_APPID);
        rec2.values = wall_record("s2", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent2.common_mut().extended_data.add_record(rec2);
        let w2 = scene.add_entity(ent2);
        regenerate_wall_representation(&mut scene, w2, None).expect("regen w2");

        // w3: vertical from (0,0) to (0,-5)
        let mut pl3 = LwPolyline::new();
        pl3.add_vertex(LwVertex::new(Vector2::new(0.0, 0.0)));
        pl3.add_vertex(LwVertex::new(Vector2::new(0.0, -5.0)));
        let mut ent3 = EntityType::LwPolyline(pl3);
        let mut rec3 = ExtendedDataRecord::new(AEC_APPID);
        rec3.values = wall_record("s3", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent3.common_mut().extended_data.add_record(rec3);
        let w3 = scene.add_entity(ent3);
        regenerate_wall_representation(&mut scene, w3, None).expect("regen w3");

        // Junction B at (10,0): w1 + w4 + w5
        // w4: vertical from (10,0) to (10,5)
        let mut pl4 = LwPolyline::new();
        pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl4.add_vertex(LwVertex::new(Vector2::new(10.0, 5.0)));
        let mut ent4 = EntityType::LwPolyline(pl4);
        let mut rec4 = ExtendedDataRecord::new(AEC_APPID);
        rec4.values = wall_record("s4", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent4.common_mut().extended_data.add_record(rec4);
        let w4 = scene.add_entity(ent4);
        regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");

        // w5: vertical from (10,0) to (10,-5)
        let mut pl5 = LwPolyline::new();
        pl5.add_vertex(LwVertex::new(Vector2::new(10.0, 0.0)));
        pl5.add_vertex(LwVertex::new(Vector2::new(10.0, -5.0)));
        let mut ent5 = EntityType::LwPolyline(pl5);
        let mut rec5 = ExtendedDataRecord::new(AEC_APPID);
        rec5.values = wall_record("s5", 3.0, 0, &layers, &[], WallJustification::Center, PlanPhase::New, None);
        ent5.common_mut().extended_data.add_record(rec5);
        let w5 = scene.add_entity(ent5);
        regenerate_wall_representation(&mut scene, w5, None).expect("regen w5");

        // Join Junction A.
        let junc_a_handles = vec![w1, w2, w3];
        let touched_a = join_junction_in_document(&mut scene,  &junc_a_handles,  None,  None,  None,  None).expect("join A");
        assert!(touched_a.contains(&w1));
        assert!(touched_a.contains(&w2));
        assert!(touched_a.contains(&w3));

        // Join Junction B.
        let junc_b_handles = vec![w1, w4, w5];
        let touched_b = join_junction_in_document(&mut scene,  &junc_b_handles,  None,  None,  None,  None).expect("join B");
        assert!(touched_b.contains(&w1));
        assert!(touched_b.contains(&w4));
        assert!(touched_b.contains(&w5));

        // Assert both junctions' participants have mitered footprints.
        for h in &[w1, w2, w3, w4, w5] {
            let wall = wall_from_entity(scene.document.get_entity(*h).unwrap()).unwrap();
            // N-way junction should produce derived handles for mitered layers.
            assert!(!wall.derived_handles.is_empty(), "wall {} should have mitered footprints", h.value());
        }

        // Assert peers_of is correct.
        let peers_w1 = engine::owner_index::peers_of(&scene.document, w1);
        assert!(peers_w1.contains(&w2));
        assert!(peers_w1.contains(&w3));
        assert!(peers_w1.contains(&w4));
        assert!(peers_w1.contains(&w5));
        assert_eq!(peers_w1.len(), 4);

        let peers_w2 = engine::owner_index::peers_of(&scene.document, w2);
        assert_eq!(peers_w2.len(), 2);
        assert!(peers_w2.contains(&w1));
        assert!(peers_w2.contains(&w3));

        // Test the safety guard: passing all handles at once should yield Ambiguous.
        let all_handles = vec![w1, w2, w3, w4, w5];
        let result = join_junction_in_document(&mut scene,  &all_handles,  None,  None,  None,  None);
        assert_eq!(result.err(), Some(JoinError::Ambiguous));
    }

    #[test]
    fn vertex_move_cascades_to_full_junction() {
        let mut scene = Scene::new();
        // 3-way junction at (0,0).
        // W1: (0,0) to (10,0)
        // W2: (0,0) to (0,10)
        // W3: (0,0) to (0,-10)

        let w1 = add_multi_layer_wall(&mut scene);
        update_wall_vertices(&mut scene, w1, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0)]);
        regenerate_wall_representation(&mut scene, w1, None).unwrap();

        let w2 = add_multi_layer_wall(&mut scene);
        update_wall_vertices(&mut scene, w2, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, 10.0, 0.0)]);
        regenerate_wall_representation(&mut scene, w2, None).unwrap();

        let w3 = add_multi_layer_wall(&mut scene);
        update_wall_vertices(&mut scene, w3, &[DVec3::new(0.0, 0.0, 0.0), DVec3::new(0.0, -10.0, 0.0)]);
        regenerate_wall_representation(&mut scene, w3, None).unwrap();

        // Initial join.
        join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).unwrap();

        // Move W1's endpoint at (0,0) slightly to (0.1, 0.1).
        // This should trigger a rebuild of ALL THREE walls via try_auto_join_nearby_walls.
        let mut axis1 = get_wall_vertices(&scene, w1);
        axis1[0] = DVec3::new(0.1, 0.0, 0.0);
        update_wall_vertices(&mut scene, w1, &axis1);
        regenerate_wall_representation(&mut scene, w1, None).unwrap();
        
        let touched = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);

        // Assert all 3 walls are still joined (their ends moved to (0.1, 0.0)).
        let axis1 = get_wall_vertices(&scene, w1);
        assert!((axis1[0].x - 0.1).abs() < 1e-6);

        // If it worked, W2 and W3 should also have their ends moved to (0.1, 0.0).
        let axis2 = get_wall_vertices(&scene, w2);
        let axis3 = get_wall_vertices(&scene, w3);

        assert!(
            (axis2[0].x - 0.1).abs() < 1e-6,
            "W2 should have followed W1 move to (0.1, 0), got {:?}",
            axis2[0]
        );
        assert!(
            (axis3[0].x - 0.1).abs() < 1e-6,
            "W3 should have followed W1 move to (0.1, 0), got {:?}",
            axis3[0]
        );
        assert!(touched.contains(&w1));
        assert!(touched.contains(&w2));
        assert!(touched.contains(&w3));

        // Assert peer links are still correct.
        let peers1 = engine::owner_index::peers_of(&scene.document, w1);
        assert!(peers1.contains(&w2));
        assert!(peers1.contains(&w3));
        assert_eq!(peers1.len(), 2);

        // Now move W1 far away and assert unlinking.
        let mut axis1 = get_wall_vertices(&scene, w1);
        axis1[0] = DVec3::new(100.0, 100.0, 0.0);
        axis1[1] = DVec3::new(110.0, 100.0, 0.0);
        update_wall_vertices(&mut scene, w1, &axis1);
        
        let touched_far = try_auto_join_nearby_walls(&mut scene,  w1,  None,  None,  None);
        
        let peers1_far = engine::owner_index::peers_of(&scene.document, w1);
        assert!(peers1_far.is_empty(), "W1 should be unlinked after moving far away");
        assert!(touched_far.contains(&w2));
        assert!(touched_far.contains(&w3));
        
        let peers2_far = engine::owner_index::peers_of(&scene.document, w2);
        assert!(!peers2_far.contains(&w1));
        assert!(peers2_far.contains(&w3)); // W2 and W3 still meet at (0.1, 0)
    }

    /// Collects every vertex of every `LwPolyline` derived (`WALL_REP`)
    /// child of `wall_handle` into one flat list, for corner-position
    /// assertions against a wall's rendered footprint(s).
    fn wall_contour_points(scene: &Scene, wall_handle: Handle) -> Vec<(f64, f64)> {
        let Some(entity) = scene.document.get_entity(wall_handle) else {
            return Vec::new();
        };
        let Some(wall) = wall_from_entity(entity) else {
            return Vec::new();
        };
        let mut pts = Vec::new();
        for h in &wall.derived_handles {
            if let Some(EntityType::LwPolyline(pl)) = scene.document.get_entity(*h) {
                pts.extend(pl.vertices.iter().map(|v| (v.location.x, v.location.y)));
            }
        }
        pts
    }

    fn has_point(pts: &[(f64, f64)], target: (f64, f64), tol: f64) -> bool {
        pts.iter()
            .any(|p| (p.0 - target.0).abs() < tol && (p.1 - target.1).abs() < tol)
    }

    fn add_single_layer_wall(scene: &mut Scene, p1: (f64, f64), p2: (f64, f64)) -> Handle {
        add_layered_wall(scene, p1, p2, vec![wl("Concrete", 0.2, "Structural")])
    }

    fn add_layered_wall(
        scene: &mut Scene,
        p1: (f64, f64),
        p2: (f64, f64),
        layers: Vec<WallLayer>,
    ) -> Handle {
        let mut pl = LwPolyline::new();
        pl.add_vertex(LwVertex::new(Vector2::new(p1.0, p1.1)));
        pl.add_vertex(LwVertex::new(Vector2::new(p2.0, p2.1)));
        let mut entity = EntityType::LwPolyline(pl);
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = wall_record(
            "style1",
            3.0,
            0,
            &layers,
            &[],
            WallJustification::Center,
            PlanPhase::New,
            None,
        );
        entity.common_mut().extended_data.add_record(record);
        scene.add_entity(entity)
    }

    /// Regression test for the "other end's join reverts to a plain cap"
    /// bug: Wall A is joined to Wall B at A's end 1 (10,0), producing a
    /// correctly mitered L-corner there (exact corner coordinates match the
    /// `l_corner_miter_shares_diagonal_endpoints` geometry in `miter.rs`).
    /// A NEW Wall C is then joined to A's *other* end (0,0). After that
    /// second join, A's rendered footprint must still contain the original
    /// A-B miter corners *and* a real (non-plain-cap) miter at the A-C end.
    #[test]
    fn other_end_join_survives_new_join_at_opposite_end() {
        let mut scene = Scene::new();
        let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
        regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

        join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

        // Plain (un-joined) end-0 cap footprint corners for reference.
        let plain_end0 = [(0.0, -0.1), (0.0, 0.1)];
        let corner1 = (10.1, -0.1);
        let corner2 = (9.9, 0.1);

        let pts_before = wall_contour_points(&scene, wall_a);
        assert!(
            has_point(&pts_before, corner1, 1e-6) && has_point(&pts_before, corner2, 1e-6),
            "A-B miter corners must be present right after the first join, got {pts_before:?}"
        );

        // NEW wall C joins A's other end (0,0).
        let wall_c = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, -10.0));
        regenerate_wall_representation(&mut scene, wall_c, None).expect("regen c");
        join_two_walls_in_document(&mut scene,  wall_a,  wall_c,  None,  None,  None).expect("A-C join");

        let pts_after = wall_contour_points(&scene, wall_a);
        assert!(
            has_point(&pts_after, corner1, 1e-6) && has_point(&pts_after, corner2, 1e-6),
            "A-B miter corners must survive regeneration triggered by the NEW A-C join, got {pts_after:?}"
        );
        assert!(
            !plain_end0.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
            "end 0 must show a real miter against C, not the plain unjoined cap, got {pts_after:?}"
        );
    }

    /// N-way variant of the regression above: three walls already meet at
    /// one point via `join_junction_in_document`; a fourth wall then joins
    /// one of them at *its other end*. Both ends of the middle wall must
    /// stay correctly joined afterward.
    #[test]
    fn n_way_junction_survives_new_join_at_participants_other_end() {
        let mut scene = Scene::new();
        // Genuine (non-collinear) 3-way junction at (0,0): w1 along +X,
        // w2 along +Y, w3 along a third direction — each participant gets
        // a real diagonal miter at the shared point, not a straight-through
        // continuation.
        let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
        let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
        for h in [w1, w2, w3] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

        // Snapshot every vertex near the junction corner (close to the
        // origin) before the new join is introduced.
        let pts_before = wall_contour_points(&scene, w1);
        let near_origin_before: Vec<(f64, f64)> =
            pts_before.into_iter().filter(|p| p.0 < 5.0).collect();
        assert!(
            !near_origin_before.is_empty(),
            "sanity: w1 should have a real N-way miter near the junction"
        );
        let plain_end1 = [(10.0, -0.1), (10.0, 0.1)];
        assert!(
            plain_end1.iter().all(|p| has_point(&wall_contour_points(&scene, w1), *p, 1e-6)),
            "sanity: w1's un-joined end should still be a plain cap before the new join"
        );

        // NEW wall w4 joins w1 at ITS other end (10,0).
        let w4 = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
        regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
        join_two_walls_in_document(&mut scene,  w1,  w4,  None,  None,  None).expect("w1-w4 join");

        let pts_after = wall_contour_points(&scene, w1);
        assert!(
            !plain_end1.iter().all(|p| has_point(&pts_after, *p, 1e-6)),
            "w1's new join end must be a real miter, not the plain cap"
        );
        // The original N-way junction corner geometry (near x=0) must be
        // byte-for-byte preserved after this unrelated join event elsewhere.
        for p in &near_origin_before {
            assert!(
                has_point(&pts_after, *p, 1e-9),
                "w1's original N-way junction corner point {p:?} must survive the new A-B join, got {pts_after:?}"
            );
        }
    }

    /// A wall with only ONE join (no second join at all) must keep producing
    /// exactly the same mitered footprint as before this change — no
    /// accidental behavior change for the common single-join case.
    #[test]
    fn single_join_wall_footprint_unchanged() {
        let mut scene = Scene::new();
        let wall_a = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let wall_b = add_single_layer_wall(&mut scene, (10.0, 0.0), (10.0, 10.0));
        regenerate_wall_representation(&mut scene, wall_a, None).expect("regen a");
        regenerate_wall_representation(&mut scene, wall_b, None).expect("regen b");

        join_two_walls_in_document(&mut scene,  wall_a,  wall_b,  None,  None,  None).expect("A-B join");

        let pts = wall_contour_points(&scene, wall_a);
        let corner1 = (10.1, -0.1);
        let corner2 = (9.9, 0.1);
        assert!(has_point(&pts, corner1, 1e-6) && has_point(&pts, corner2, 1e-6));

        // Plain cap at the un-joined end 0 must be exactly the un-mitered
        // rectangle corners (no peer exists there).
        assert!(has_point(&pts, (0.0, -0.1), 1e-6));
        assert!(has_point(&pts, (0.0, 0.1), 1e-6));
    }

    /// Regression test for the reported bug: three walls already meet at one
    /// point via an N-way junction (w1, w2, w3, all correctly mitered).
    /// A NEW wall w4 is then joined to the *same* junction point. After the
    /// junction is re-resolved for 4 participants, the *other* walls (w2, w3)
    /// — which did not change themselves — must still show their correct
    /// mitered footprint, not revert to a plain unjoined cap.
    #[test]
    fn adding_new_wall_to_existing_junction_keeps_other_walls_mitered() {
        let mut scene = Scene::new();
        let w1 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, 0.0));
        let w2 = add_single_layer_wall(&mut scene, (0.0, 0.0), (0.0, 10.0));
        let w3 = add_single_layer_wall(&mut scene, (0.0, 0.0), (-10.0, 10.0));
        for h in [w1, w2, w3] {
            regenerate_wall_representation(&mut scene, h, None).expect("initial regen");
        }
        join_junction_in_document(&mut scene,  &[w1, w2, w3],  None,  None,  None,  None).expect("N-way join");

        // Plain (un-joined) cap corners at the origin end of a vertical /
        // diagonal single-layer (0.2 thick) wall — what w2/w3 would show at
        // their origin end if the junction miter were lost and they fell
        // back to an un-joined rectangle cap.
        let plain_origin_cap = [(-0.1, 0.0), (0.1, 0.0)];

        let w2_before = wall_contour_points(&scene, w2);
        let w3_before = wall_contour_points(&scene, w3);
        assert!(
            !plain_origin_cap.iter().all(|p| has_point(&w2_before, *p, 1e-6)),
            "sanity: w2 should have a real N-way miter, not a plain cap, got {w2_before:?}"
        );
        assert!(
            !plain_origin_cap.iter().all(|p| has_point(&w3_before, *p, 1e-6)),
            "sanity: w3 should have a real N-way miter, not a plain cap, got {w3_before:?}"
        );

        // NEW wall w4 joins the SAME junction point (0,0), the way the
        // interactive drawing workflow actually triggers it: via
        // `try_auto_join_nearby_walls` for just the newly drawn wall.
        let w4 = add_single_layer_wall(&mut scene, (0.0, 0.0), (10.0, -10.0));
        regenerate_wall_representation(&mut scene, w4, None).expect("regen w4");
        try_auto_join_nearby_walls(&mut scene,  w4,  None,  None,  None);

        let w2_after = wall_contour_points(&scene, w2);
        let w3_after = wall_contour_points(&scene, w3);
        assert!(
            !plain_origin_cap.iter().all(|p| has_point(&w2_after, *p, 1e-6)),
            "w2 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w2_after:?}"
        );
        assert!(
            !plain_origin_cap.iter().all(|p| has_point(&w3_after, *p, 1e-6)),
            "w3 must still show a real miter at the junction after w4 joins, not revert to a plain cap, got {w3_after:?}"
        );
    }
}
