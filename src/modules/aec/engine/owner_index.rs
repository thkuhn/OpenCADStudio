//! Generic owner/peer handle index stored as AEC XDATA on any entity.
//!
//! Two record kinds live alongside other `OPENCAD_AEC` records on the owner:
//! - `CHILD_HANDLES` — ordered list of owned child entity handles
//! - `JOINED_PEERS` — symmetric list of peer entity handles (e.g. joined walls)
//!
//! Lookups never scan the document: missing index XDATA yields an empty list.

use acadrust::tables::AppId;
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, EntityType, Handle};

/// APPID shared with the rest of the AEC module (must stay stable for round-trip).
const AEC_APPID: &str = "OPENCAD_AEC";

/// XDATA kind tag for the child-handle index on an owner entity.
pub const CHILD_HANDLES_TAG: &str = "CHILD_HANDLES";

/// XDATA kind tag for the symmetric peer-handle index on an entity.
pub const JOINED_PEERS_TAG: &str = "JOINED_PEERS";

fn ensure_app_id(doc: &mut CadDocument) {
    if !doc.app_ids.contains(AEC_APPID) {
        let mut app = AppId::new(AEC_APPID);
        app.handle = doc.allocate_handle();
        let _ = doc.app_ids.add(app);
    }
}

/// True when `record` is an `OPENCAD_AEC` record whose first value is `tag`.
fn record_has_tag(record: &ExtendedDataRecord, tag: &str) -> bool {
    if record.application_name != AEC_APPID {
        return false;
    }
    matches!(record.values.first(), Some(XDataValue::String(s)) if s == tag)
}

/// Read handle list from the first `tag` record on `entity`.
fn read_handles(entity: &EntityType, tag: &str) -> Vec<Handle> {
    let mut out = Vec::new();
    for record in entity.common().extended_data.records() {
        if !record_has_tag(record, tag) {
            continue;
        }
        for value in record.values.iter().skip(1) {
            match value {
                XDataValue::Handle(h) => out.push(*h),
                XDataValue::Integer32(v) if *v >= 0 => out.push(Handle::new(*v as u64)),
                XDataValue::Integer16(v) if *v >= 0 => out.push(Handle::new(*v as u64)),
                XDataValue::String(s) => {
                    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
                    if let Some(h) = u64::from_str_radix(s, 16)
                        .ok()
                        .or_else(|| s.parse().ok())
                    {
                        out.push(Handle::new(h));
                    }
                }
                _ => {}
            }
        }
        break;
    }
    out
}

/// Replace (or create) the `tag` handle-list record on `owner`, preserving
/// every other XDATA record on the entity (including other AEC kinds).
fn write_handles(doc: &mut CadDocument, owner: Handle, tag: &str, handles: &[Handle]) -> bool {
    ensure_app_id(doc);
    let app_handle = doc.app_ids.get(AEC_APPID).map(|a| a.handle.value());
    let Some(entity) = doc.get_entity_mut(owner) else {
        return false;
    };

    let mut record = ExtendedDataRecord::new(AEC_APPID);
    record.add_value(XDataValue::String(tag.to_string()));
    for h in handles {
        record.add_value(XDataValue::Handle(*h));
    }

    let xd = &mut entity.common_mut().extended_data;
    let kept: Vec<_> = xd
        .records()
        .iter()
        .filter(|r| !record_has_tag(r, tag))
        .cloned()
        .collect();
    xd.clear();
    for r in kept {
        xd.add_record(r);
    }
    // Empty index: drop the record entirely so missing and empty stay equivalent.
    if !handles.is_empty() {
        xd.add_record(record);
    }
    if let Some(ah) = app_handle {
        // The verbatim DWG blob is per APPID, not per record. Leaving it would
        // overwrite every structured AEC record (WALL, CHILD_HANDLES, …) on save.
        xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
    }
    true
}

/// Append `child` to `owner`'s `CHILD_HANDLES` index (no-op if already present).
pub fn add_child(doc: &mut CadDocument, owner: Handle, child: Handle) {
    if owner == child {
        return;
    }
    let Some(entity) = doc.get_entity(owner) else {
        return;
    };
    let mut children = read_handles(entity, CHILD_HANDLES_TAG);
    if children.contains(&child) {
        return;
    }
    children.push(child);
    let _ = write_handles(doc, owner, CHILD_HANDLES_TAG, &children);
}

/// Remove `child` from `owner`'s `CHILD_HANDLES` index (no-op if absent).
pub fn remove_child(doc: &mut CadDocument, owner: Handle, child: Handle) {
    let Some(entity) = doc.get_entity(owner) else {
        return;
    };
    let mut children = read_handles(entity, CHILD_HANDLES_TAG);
    let before = children.len();
    children.retain(|h| *h != child);
    if children.len() == before {
        return;
    }
    let _ = write_handles(doc, owner, CHILD_HANDLES_TAG, &children);
}

/// Handles currently listed as children of `owner`. Empty when no index exists.
pub fn children_of(doc: &CadDocument, owner: Handle) -> Vec<Handle> {
    let Some(entity) = doc.get_entity(owner) else {
        return Vec::new();
    };
    read_handles(entity, CHILD_HANDLES_TAG)
}

/// Ensure `a` and `b` each list the other under `JOINED_PEERS` (symmetric).
pub fn link_peers(doc: &mut CadDocument, a: Handle, b: Handle) {
    if a == b {
        return;
    }
    add_peer(doc, a, b);
    add_peer(doc, b, a);
}

/// Remove the symmetric `JOINED_PEERS` link between `a` and `b`.
pub fn unlink_peers(doc: &mut CadDocument, a: Handle, b: Handle) {
    if a == b {
        return;
    }
    remove_peer(doc, a, b);
    remove_peer(doc, b, a);
}

/// Handles currently listed as peers of `owner`. Empty when no index exists.
pub fn peers_of(doc: &CadDocument, owner: Handle) -> Vec<Handle> {
    let Some(entity) = doc.get_entity(owner) else {
        return Vec::new();
    };
    read_handles(entity, JOINED_PEERS_TAG)
}

fn add_peer(doc: &mut CadDocument, owner: Handle, peer: Handle) {
    let Some(entity) = doc.get_entity(owner) else {
        return;
    };
    let mut peers = read_handles(entity, JOINED_PEERS_TAG);
    if peers.contains(&peer) {
        return;
    }
    peers.push(peer);
    let _ = write_handles(doc, owner, JOINED_PEERS_TAG, &peers);
}

fn remove_peer(doc: &mut CadDocument, owner: Handle, peer: Handle) {
    let Some(entity) = doc.get_entity(owner) else {
        return;
    };
    let mut peers = read_handles(entity, JOINED_PEERS_TAG);
    let before = peers.len();
    peers.retain(|h| *h != peer);
    if peers.len() == before {
        return;
    }
    let _ = write_handles(doc, owner, JOINED_PEERS_TAG, &peers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::LwPolyline;

    fn blank_doc_with_n_entities(n: usize) -> (CadDocument, Vec<Handle>) {
        let mut doc = CadDocument::new();
        let mut handles = Vec::with_capacity(n);
        for _ in 0..n {
            handles.push(
                doc.add_entity(EntityType::LwPolyline(LwPolyline::new()))
                    .expect("add entity"),
            );
        }
        (doc, handles)
    }

    #[test]
    fn add_and_remove_child_updates_index() {
        let (mut doc, hs) = blank_doc_with_n_entities(3);
        let (owner, c1, c2) = (hs[0], hs[1], hs[2]);

        assert!(children_of(&doc, owner).is_empty());

        add_child(&mut doc, owner, c1);
        add_child(&mut doc, owner, c2);
        // duplicate is a no-op
        add_child(&mut doc, owner, c1);
        assert_eq!(children_of(&doc, owner), vec![c1, c2]);

        remove_child(&mut doc, owner, c1);
        assert_eq!(children_of(&doc, owner), vec![c2]);

        remove_child(&mut doc, owner, c2);
        assert!(children_of(&doc, owner).is_empty());
        // removing a missing child is a no-op
        remove_child(&mut doc, owner, c1);
        assert!(children_of(&doc, owner).is_empty());
    }

    #[test]
    fn reparent_child_moves_between_owners() {
        let (mut doc, hs) = blank_doc_with_n_entities(3);
        let (a, b, child) = (hs[0], hs[1], hs[2]);

        add_child(&mut doc, a, child);
        assert_eq!(children_of(&doc, a), vec![child]);
        assert!(children_of(&doc, b).is_empty());

        remove_child(&mut doc, a, child);
        add_child(&mut doc, b, child);
        assert!(children_of(&doc, a).is_empty());
        assert_eq!(children_of(&doc, b), vec![child]);
    }

    #[test]
    fn link_and_unlink_peers_are_symmetric() {
        let (mut doc, hs) = blank_doc_with_n_entities(3);
        let (a, b, c) = (hs[0], hs[1], hs[2]);

        assert!(peers_of(&doc, a).is_empty());
        assert!(peers_of(&doc, b).is_empty());

        link_peers(&mut doc, a, b);
        assert_eq!(peers_of(&doc, a), vec![b]);
        assert_eq!(peers_of(&doc, b), vec![a]);

        // second link is idempotent
        link_peers(&mut doc, a, b);
        assert_eq!(peers_of(&doc, a), vec![b]);

        link_peers(&mut doc, a, c);
        assert_eq!(peers_of(&doc, a), vec![b, c]);
        assert_eq!(peers_of(&doc, c), vec![a]);

        unlink_peers(&mut doc, a, b);
        assert_eq!(peers_of(&doc, a), vec![c]);
        assert!(peers_of(&doc, b).is_empty());
        assert_eq!(peers_of(&doc, c), vec![a]);

        unlink_peers(&mut doc, a, c);
        assert!(peers_of(&doc, a).is_empty());
        assert!(peers_of(&doc, c).is_empty());
    }

    #[test]
    fn empty_index_when_nothing_was_set() {
        let (doc, hs) = blank_doc_with_n_entities(1);
        assert!(children_of(&doc, hs[0]).is_empty());
        assert!(peers_of(&doc, hs[0]).is_empty());
        // unknown handle also empty
        assert!(children_of(&doc, Handle::new(999_999)).is_empty());
        assert!(peers_of(&doc, Handle::new(999_999)).is_empty());
    }

    #[test]
    fn child_and_peer_indexes_coexist_on_same_entity() {
        let (mut doc, hs) = blank_doc_with_n_entities(3);
        let (owner, child, peer) = (hs[0], hs[1], hs[2]);

        add_child(&mut doc, owner, child);
        link_peers(&mut doc, owner, peer);

        assert_eq!(children_of(&doc, owner), vec![child]);
        assert_eq!(peers_of(&doc, owner), vec![peer]);
        assert_eq!(peers_of(&doc, peer), vec![owner]);
    }
}
