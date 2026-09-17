//! v6U-A2 migration-only projection layer.
include!("library_effects_v1_generated.rs");

pub fn allocation_disposition_projection_for_evidence(
    evidence: &RustAllocationDispositionEvidenceKind,
) -> LegacyDispositionProjection {
    projection_for_producer_evidence(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::RustAllocationDispositionEvidenceKind::*;

    #[test]
    fn b1_1_cstring_producer_evidence_wire_names_match_protocol() {
        assert_eq!(
            serde_json::to_string(&CStringIntoRaw).unwrap(),
            "\"cstring_into_raw\""
        );
        assert_eq!(
            serde_json::to_string(&CStringFromRaw).unwrap(),
            "\"cstring_from_raw\""
        );
        assert_eq!(
            serde_json::from_str::<crate::structs::RustAllocationDispositionEvidenceKind>(
                "\"cstring_into_raw\""
            )
            .unwrap(),
            CStringIntoRaw
        );
        assert_eq!(
            serde_json::from_str::<crate::structs::RustAllocationDispositionEvidenceKind>(
                "\"cstring_from_raw\""
            )
            .unwrap(),
            CStringFromRaw
        );
        assert!(serde_json::from_str::<crate::structs::RustAllocationDispositionEvidenceKind>(
            "\"c_string_into_raw\""
        )
        .is_err());
        assert!(serde_json::from_str::<crate::structs::RustAllocationDispositionEvidenceKind>(
            "\"c_string_from_raw\""
        )
        .is_err());
    }

    #[test]
    fn v6u_a2_all_legacy_evidence_has_exact_generated_projection() {
        let cases = [
            (BoxIntoRaw,"rust_box_into_raw_v1","box_into_raw","preserve_manual_obligation","rustc_box_into_raw_v1",LegacyTargetVariable::Return),
            (BoxFromRaw,"rust_box_from_raw_v1","box_from_raw","restore_raii_obligation","rustc_box_from_raw_v1",LegacyTargetVariable::Return),
            (BoxLeak,"rust_box_leak_v1","box_leak","preserve_persistent_obligation","rustc_box_leak_v1",LegacyTargetVariable::Return),
            (CStringIntoRaw,"rust_cstring_into_raw_v1","cstring_into_raw","preserve_manual_obligation","rustc_cstring_into_raw_v1",LegacyTargetVariable::Return),
            (CStringFromRaw,"rust_cstring_from_raw_v1","cstring_from_raw","restore_raii_obligation","rustc_cstring_from_raw_v1",LegacyTargetVariable::Return),
            (MemForgetOwnedBox,"rust_mem_forget_owned_box_v1","mem_forget_owned_box","preserve_unreclaimed_obligation","rustc_mem_forget_owned_box_v1",LegacyTargetVariable::None),
            (MemDropRawPointer,"rust_mem_drop_raw_pointer_v1","raw_pointer_drop_noop","no_pointee_lifecycle_effect","rustc_mem_drop_raw_pointer_v1",LegacyTargetVariable::None),
        ];
        for (evidence,summary_id,kind,effect,basis,target) in cases {
            let p=allocation_disposition_projection_for_evidence(&evidence);
            assert_eq!(p.summary_id,summary_id); assert_eq!(p.kind,kind);
            assert_eq!(p.obligation_effect,effect); assert_eq!(p.basis,basis);
            assert_eq!(p.target_variable,target);
        }
    }
}
