//! XDATA serialization and deserialization for AEC `Room` entities.

use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

use super::plan_view::PlanPhase;
use super::room::{Room, RoomFinish, RoomFunction};
use super::xdata::{write_aec_record, AEC_APPID};

fn aec_value_as_string(v: &XDataValue) -> Option<String> {
    match v {
        XDataValue::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn aec_value_as_f64(v: &XDataValue) -> Option<f64> {
    match v {
        XDataValue::Real(r) | XDataValue::Distance(r) => Some(*r),
        _ => None,
    }
}

fn aec_value_as_i32(v: &XDataValue) -> Option<i32> {
    match v {
        XDataValue::Integer32(i) => Some(*i),
        XDataValue::Integer16(i) => Some(*i as i32),
        _ => None,
    }
}

/// Tag for primary room XDATA record.
pub const ROOM_TAG: &str = "ROOM";

/// Serializes a `Room` struct into an XDATA values vector.
pub fn room_record(room: &Room) -> Vec<XDataValue> {
    let mut values = vec![
        XDataValue::String(ROOM_TAG.to_string()),
        XDataValue::String(room.name.clone()),
        XDataValue::Real(room.area),
        XDataValue::Real(room.perimeter),
        XDataValue::Real(room.volume),
        XDataValue::Integer32(room.storey_id as i32),
        XDataValue::String(room.number.clone()),
        XDataValue::String(room.function.as_str().to_string()),
        XDataValue::Real(room.clear_height),
        XDataValue::Real(room.factor),
        XDataValue::Real(room.calculated_area()),
        XDataValue::Real(room.base_z),
        XDataValue::Integer32(room.phase as i32),
    ];

    if let Some((sx, sy)) = room.stamp_pos {
        values.push(XDataValue::Real(sx));
        values.push(XDataValue::Real(sy));
    } else {
        values.push(XDataValue::Real(f64::NAN));
        values.push(XDataValue::Real(f64::NAN));
    }

    if let Some(finishes) = &room.floor_finish {
        if let Ok(json) = serde_json::to_string(finishes) {
            values.push(XDataValue::String(json));
        } else {
            values.push(XDataValue::String(String::new()));
        }
    } else {
        values.push(XDataValue::String(String::new()));
    }

    if let Some(finishes) = &room.ceiling_finish {
        if let Ok(json) = serde_json::to_string(finishes) {
            values.push(XDataValue::String(json));
        } else {
            values.push(XDataValue::String(String::new()));
        }
    } else {
        values.push(XDataValue::String(String::new()));
    }

    if let Some(style_id) = &room.finish_style_id {
        values.push(XDataValue::String(style_id.clone()));
    } else {
        values.push(XDataValue::String(String::new()));
    }

    values
}

/// Writes a `Room` record to the given entity in `doc`.
pub fn write_room_record(doc: &mut CadDocument, handle: Handle, room: &Room) -> bool {
    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.values = room_record(room);
    write_aec_record(doc, handle, record)
}

/// Reads a `Room` struct from an entity's XDATA.
pub fn room_from_entity(entity: &EntityType) -> Option<Room> {
    for record in entity.common().extended_data.records() {
        if record.application_name != AEC_APPID {
            continue;
        }
        let tag = match record.values.first() {
            Some(XDataValue::String(s)) => s.as_str(),
            _ => continue,
        };
        if tag != ROOM_TAG {
            continue;
        }

        let name = record
            .values
            .get(1)
            .and_then(aec_value_as_string)
            .unwrap_or_else(|| "Room".to_string());
        let area = record.values.get(2).and_then(aec_value_as_f64).unwrap_or(0.0);
        let perimeter = record.values.get(3).and_then(aec_value_as_f64).unwrap_or(0.0);
        let _volume = record.values.get(4).and_then(aec_value_as_f64).unwrap_or(0.0);
        let storey_id = record
            .values
            .get(5)
            .and_then(aec_value_as_i32)
            .unwrap_or(0)
            .max(0) as u32;

        let number = record
            .values
            .get(6)
            .and_then(aec_value_as_string)
            .unwrap_or_default();
        let function_str = record
            .values
            .get(7)
            .and_then(aec_value_as_string)
            .unwrap_or_else(|| "Living".to_string());
        let function = RoomFunction::from_str(&function_str);
        let clear_height = record
            .values
            .get(8)
            .and_then(aec_value_as_f64)
            .unwrap_or(2.50);
        let factor = record
            .values
            .get(9)
            .and_then(aec_value_as_f64)
            .unwrap_or_else(|| function.default_factor());
        let _calc_area = record
            .values
            .get(10)
            .and_then(aec_value_as_f64)
            .unwrap_or(area * factor);
        let base_z = record
            .values
            .get(11)
            .and_then(aec_value_as_f64)
            .unwrap_or(0.0);
        let phase = record
            .values
            .get(12)
            .and_then(aec_value_as_i32)
            .and_then(|v| match v {
                0 => Some(PlanPhase::Existing),
                1 => Some(PlanPhase::Demolition),
                2 => Some(PlanPhase::New),
                _ => None,
            })
            .unwrap_or(PlanPhase::New);

        let stamp_pos = if let (Some(sx), Some(sy)) = (
            record.values.get(13).and_then(aec_value_as_f64),
            record.values.get(14).and_then(aec_value_as_f64),
        ) {
            if sx.is_nan() || sy.is_nan() {
                None
            } else {
                Some((sx, sy))
            }
        } else {
            None
        };

        let floor_finish = record
            .values
            .get(15)
            .and_then(aec_value_as_string)
            .and_then(|s| {
                if s.trim().is_empty() {
                    None
                } else {
                    serde_json::from_str::<Vec<RoomFinish>>(&s).ok()
                }
            });

        let ceiling_finish = record
            .values
            .get(16)
            .and_then(aec_value_as_string)
            .and_then(|s| {
                if s.trim().is_empty() {
                    None
                } else {
                    serde_json::from_str::<Vec<RoomFinish>>(&s).ok()
                }
            });

        let finish_style_id = record
            .values
            .get(17)
            .and_then(aec_value_as_string)
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

        let (area, perimeter) = if let EntityType::LwPolyline(pl) = entity {
            let mut points: Vec<(f64, f64)> = pl
                .vertices
                .iter()
                .map(|v| (v.location.x, v.location.y))
                .collect();
            if points.len() >= 3 {
                let first = points[0];
                let last = points[points.len() - 1];
                if (first.0 - last.0).hypot(first.1 - last.1) < 1e-6 {
                    points.pop();
                }
                if points.len() >= 3 {
                    (
                        super::geometry::area(&points),
                        super::geometry::perimeter(&points),
                    )
                } else {
                    (area, perimeter)
                }
            } else {
                (area, perimeter)
            }
        } else {
            (area, perimeter)
        };
        let volume = area * factor * clear_height;

        return Some(Room {
            name,
            number,
            function,
            area,
            factor,
            perimeter,
            clear_height,
            volume,
            storey_id,
            base_z,
            phase,
            stamp_pos,
            finish_style_id,
            floor_finish,
            ceiling_finish,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwPolyline;

    #[test]
    fn test_room_xdata_roundtrip_with_finishes() {
        let room = Room {
            name: "Kitchen".to_string(),
            number: "K-02".to_string(),
            function: RoomFunction::Kitchen,
            area: 15.5,
            factor: 1.0,
            perimeter: 16.0,
            clear_height: 2.6,
            volume: 40.3,
            storey_id: 1,
            base_z: 2.8,
            phase: PlanPhase::New,
            stamp_pos: Some((2.5, 3.0)),
            finish_style_id: Some("finish_floor_tiles_80".to_string()),
            floor_finish: Some(vec![RoomFinish::new("Tiles", 0.015).with_hatch("SQUARE")]),
            ceiling_finish: Some(vec![RoomFinish::new("Plaster", 0.015)]),
        };

        let values = room_record(&room);
        let mut entity = EntityType::LwPolyline(LwPolyline::new());
        let mut record = ExtendedDataRecord::new(AEC_APPID);
        record.values = values;
        entity.common_mut().extended_data.add_record(record);

        let parsed = room_from_entity(&entity).expect("Should parse room");
        assert_eq!(parsed.name, "Kitchen");
        assert_eq!(parsed.number, "K-02");
        assert_eq!(parsed.finish_style_id.as_deref(), Some("finish_floor_tiles_80"));
        assert_eq!(parsed.floor_finish_summary(), "finish_floor_tiles_80");
        assert_eq!(parsed.ceiling_finish_summary(), "Plaster");
        assert_eq!(parsed.stamp_pos, Some((2.5, 3.0)));
    }
}
