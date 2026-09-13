//! Canonical Rust memory-event classification used by the abstract domain,
//! allocation identity analysis, and CQPL exporter.
//!
//! Scientific invariant: an operation is classified in one place only.  In
//! particular, ownership-preserving conversions such as `into_vec` are not
//! fresh allocation sites, while low-level allocation primitives such as
//! `exchange_malloc` are.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustAllocationSemantics {
    None,
    /// A fresh backing allocation exists on every normal return represented by
    /// the call summary.
    Fresh,
    /// On a normal return the result may either own freshly allocated storage
    /// or reuse/consume storage supplied by an input value.  Identity therefore
    /// joins a fresh site with source identities; event semantics is MAY.
    MayFreshOrTransfer,
    /// Ownership/representation changes while the backing allocation identity
    /// is preserved.
    Transfer,
}

fn has_method(s: &str, method: &str) -> bool {
    let plain = format!("::{method}");
    s.ends_with(&plain) || s.contains(&format!("::{method}::<"))
}

pub fn is_exchange_malloc_call(s: &str) -> bool {
    s.contains("alloc::alloc::exchange_malloc")
        || s.contains("std::alloc::exchange_malloc")
}

pub fn is_into_vec_transfer_call(s: &str) -> bool {
    (s.contains("std::slice::<impl [") || s.contains("alloc::slice::<impl ["))
        && s.contains(">::into_vec::<")
}

fn is_box_new_call(s: &str) -> bool {
    (s.contains("std::boxed::Box::<") || s.contains("alloc::boxed::Box::<"))
        && has_method(s, "new")
}

fn is_raw_alloc_zeroed_call(s: &str) -> bool {
    s.contains("std::alloc::alloc_zeroed") || s.contains("alloc::alloc::alloc_zeroed")
}

fn is_raw_alloc_call(s: &str) -> bool {
    (s.contains("std::alloc::alloc") || s.contains("alloc::alloc::alloc"))
        && !is_raw_alloc_zeroed_call(s)
        && !s.contains("handle_alloc_error")
        && !s.contains("dealloc")
}

fn is_cstring_new(s: &str) -> bool {
    s.contains("CString") && has_method(s, "new")
}

fn is_cstring_from_cstr(s: &str) -> bool {
    s.contains("<std::ffi::CString as std::convert::From<&std::ffi::CStr>>::from")
        || s.contains("<alloc::ffi::c_str::CString as core::convert::From<&core::ffi::c_str::CStr>>::from")
}

pub fn rust_allocation_semantics(s: &str) -> RustAllocationSemantics {
    if is_into_vec_transfer_call(s) {
        return RustAllocationSemantics::Transfer;
    }

    if is_exchange_malloc_call(s)
        || is_box_new_call(s)
        || is_raw_alloc_call(s)
        || is_raw_alloc_zeroed_call(s)
        || is_cstring_from_cstr(s)
    {
        return RustAllocationSemantics::Fresh;
    }

    if is_cstring_new(s) {
        // CString::new<&str> necessarily creates owned CString storage on its
        // Ok path.  For generic Into<Vec<u8>> inputs, existing storage may be
        // reused; represent both alternatives as MAY identity information.
        if s.contains("::new::<&str>") {
            RustAllocationSemantics::Fresh
        } else {
            RustAllocationSemantics::MayFreshOrTransfer
        }
    } else {
        RustAllocationSemantics::None
    }
}

/// Whether a CQPL `alloc` MAY event is semantically justified at this call.
/// Both `Fresh` and `MayFreshOrTransfer` qualify; `Transfer` explicitly does
/// not, preventing the historical `into_vec` false fresh-allocation event.
pub fn is_modeled_fresh_allocation(s: &str) -> bool {
    matches!(
        rust_allocation_semantics(s),
        RustAllocationSemantics::Fresh | RustAllocationSemantics::MayFreshOrTransfer
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_malloc_is_fresh() {
        assert_eq!(
            rust_allocation_semantics("alloc::alloc::exchange_malloc"),
            RustAllocationSemantics::Fresh
        );
    }

    #[test]
    fn into_vec_is_transfer_not_fresh() {
        let call = "std::slice::<impl [Numbers]>::into_vec::<std::alloc::Global>";
        assert_eq!(rust_allocation_semantics(call), RustAllocationSemantics::Transfer);
        assert!(!is_modeled_fresh_allocation(call));
    }

    #[test]
    fn cstring_generic_input_is_may_fresh_or_transfer() {
        assert_eq!(
            rust_allocation_semantics("std::ffi::CString::new::<Vec<u8>>"),
            RustAllocationSemantics::MayFreshOrTransfer
        );
    }
}
