use acadrust::{CadDocument, Handle};
use codec::objects::ObjectType;

fn native_scale_in_document(doc: &codec::Document, entity: Handle) -> Option<Handle> {
    let extension_dictionary = doc.get_entity(entity)?.common().xdictionary_handle?;
    let manager = match doc.objects.get(&extension_dictionary)? {
        ObjectType::Dictionary(dictionary) => dictionary.get("AcDbContextDataManager")?,
        _ => return None,
    };
    let contexts = match doc.objects.get(&manager)? {
        ObjectType::Dictionary(dictionary) => dictionary.get("ACDB_ANNOTATIONSCALES")?,
        _ => return None,
    };
    let ObjectType::Dictionary(contexts) = doc.objects.get(&contexts)? else {
        return None;
    };

    let mut first_scale = None;
    for (_, context_handle) in &contexts.entries {
        if let Some(ObjectType::ObjectContextData(context)) = doc.objects.get(context_handle) {
            first_scale.get_or_insert(context.scale);
            if context.is_default {
                return Some(context.scale);
            }
        }
    }
    first_scale
}

/// Returns the native annotation scale recorded on an entity's default context.
pub fn native_annotation_scale(doc: &CadDocument, entity: Handle) -> Option<Handle> {
    let core_doc: codec::Document = doc.clone().into();
    native_scale_in_document(&core_doc, entity)
}

fn retained_scale_handle(doc: &codec::Document, retained_scale: Option<Handle>) -> Option<Handle> {
    retained_scale.filter(|handle| {
        matches!(
            doc.objects.get(handle),
            Some(ObjectType::Scale(scale)) if !scale.is_temporary
        )
    })
}

fn initial_scale_handle(doc: &mut codec::Document) -> Handle {
    let current_name = doc.header.current_annotation_scale.trim();
    if !current_name.is_empty() {
        if let Some(handle) = crate::scene::annotative::scale_handle_by_name(doc, current_name) {
            if matches!(
                doc.objects.get(&handle),
                Some(ObjectType::Scale(scale))
                    if !scale.is_temporary && (scale.factor() - 1.0).abs() > 1.0e-9
            ) {
                return handle;
            }
        }
    }

    let default_scale = codec::objects::Scale::new("1:100", 1.0, 100.0);
    crate::scene::annotative::ensure_scale_object(doc, &default_scale)
}

/// Marks an MTEXT as annotative and gives it one native annotation context.
/// A retained scale takes precedence so regeneration does not change its paper size.
pub fn bind_annotative_mtext(
    doc: &mut CadDocument,
    entity: Handle,
    retained_scale: Option<Handle>,
) -> bool {
    if !matches!(doc.get_entity(entity), Some(codec::EntityType::MText(_))) {
        return false;
    }

    let mut core_doc: codec::Document = std::mem::replace(doc, CadDocument::new()).into();
    let scale_handle = retained_scale_handle(&core_doc, retained_scale)
        .unwrap_or_else(|| initial_scale_handle(&mut core_doc));
    crate::scene::annotative::set_entity_annotative(&mut core_doc, entity, true);
    let bound =
        crate::scene::annotative::create_annotation_context(&mut core_doc, entity, scale_handle);
    *doc = core_doc.into();
    bound
}

#[cfg(test)]
mod tests {
    use super::{bind_annotative_mtext, native_annotation_scale};
    use acadrust::entities::MText;
    use acadrust::{CadDocument, EntityType};

    fn document_with_text(current_scale: &str) -> (CadDocument, acadrust::Handle) {
        let mut doc = CadDocument::new();
        doc.header.current_annotation_scale = current_scale.to_string();
        let handle = doc.allocate_handle();
        let mut entity = EntityType::MText(Box::new(MText::new()));
        entity.common_mut().handle = handle;
        doc.add_entity(entity).unwrap();
        (doc, handle)
    }

    fn add_scale(doc: &mut CadDocument, name: &str, drawing_units: f64) -> acadrust::Handle {
        let mut core_doc: codec::Document = std::mem::replace(doc, CadDocument::new()).into();
        let source = codec::objects::Scale::new(name, 1.0, drawing_units);
        let handle = crate::scene::annotative::ensure_scale_object(&mut core_doc, &source);
        *doc = core_doc.into();
        handle
    }

    fn scale_name(doc: &CadDocument, handle: acadrust::Handle) -> Option<String> {
        let core_doc: codec::Document = doc.clone().into();
        match core_doc.objects.get(&handle) {
            Some(codec::objects::ObjectType::Scale(scale)) => Some(scale.name.clone()),
            _ => None,
        }
    }

    #[test]
    fn binds_to_the_current_non_one_to_one_scale_without_changing_height() {
        let (mut doc, text) = document_with_text("1:50");
        let scale = add_scale(&mut doc, "1:50", 50.0);
        if let Some(EntityType::MText(mtext)) = doc.get_entity_mut(text) {
            mtext.height = 0.22;
        }

        assert!(bind_annotative_mtext(&mut doc, text, None));
        assert_eq!(native_annotation_scale(&doc, text), Some(scale));
        assert!(matches!(
            doc.get_entity(text),
            Some(EntityType::MText(mtext)) if mtext.is_annotative && (mtext.height - 0.22).abs() < 1.0e-9
        ));
    }

    #[test]
    fn defaults_to_one_hundred_when_current_scale_is_missing_or_one_to_one() {
        for current_scale in ["", "1:1"] {
            let (mut doc, text) = document_with_text(current_scale);
            assert!(bind_annotative_mtext(&mut doc, text, None));
            let scale = native_annotation_scale(&doc, text).expect("native context");
            assert_eq!(scale_name(&doc, scale).as_deref(), Some("1:100"));
        }
    }

    #[test]
    fn binding_again_is_idempotent_for_the_same_native_scale() {
        let (mut doc, text) = document_with_text("1:50");
        let scale = add_scale(&mut doc, "1:50", 50.0);
        assert!(bind_annotative_mtext(&mut doc, text, None));
        assert!(bind_annotative_mtext(&mut doc, text, None));
        assert_eq!(native_annotation_scale(&doc, text), Some(scale));

        let core_doc: codec::Document = doc.into();
        assert_eq!(
            crate::scene::annotative::object_scale_memberships(&core_doc, text).len(),
            1
        );
    }
}
