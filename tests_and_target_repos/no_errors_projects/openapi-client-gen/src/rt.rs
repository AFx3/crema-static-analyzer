#![allow(dead_code)]
#![allow(unused_imports)]

use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Display, format, Formatter, Write};
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use json::JsonValue;
use json::object::*;
use json::number::Number;
use linked_hash_map::LinkedHashMap;
use std::{io, mem, ptr};
use std::cell::OnceCell;
use std::cmp::min;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::{c_char, c_void, CStr};
#[cfg(any(feature = "async", target_arch = "wasm32"))]
use std::future::Future;
#[cfg(any(feature = "async", target_arch = "wasm32"))]
use std::sync::atomic::AtomicBool;

#[doc(hidden)]
macro_rules! option_wrapper {
    ($wrapper_name:ident, $inner_type:ty) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
        pub struct $wrapper_name(pub Option<$inner_type>);

        impl Deref for $wrapper_name {
            type Target = Option<$inner_type>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $wrapper_name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl From<$inner_type> for $wrapper_name {
            fn from(value: $inner_type) -> Self {
                Self(Some(value))
            }
        }

        impl From<Option<$inner_type>> for $wrapper_name {
            fn from(value: Option<$inner_type>) -> Self {
                Self(value)
            }
        }

        impl TryFrom<&JsonValue> for $wrapper_name {
            type Error = String;

            fn try_from(value: &JsonValue) -> Result<Self, String> {
                if value.is_null() {
                    return Ok(Self(None))
                }

                Ok(Self(Some(<$inner_type>::try_from(value)?)))
            }
        }

        impl Into<JsonValue> for &$wrapper_name {
            fn into(self) -> JsonValue {
                if self.0.is_none() {
                    return JsonValue::Null
                }

                self.0.as_ref().unwrap().into()
            }
        }


        impl Into<JsonValue> for $wrapper_name {
            fn into(self) -> JsonValue {
                if self.0.is_none() {
                    return JsonValue::Null
                }

                self.0.unwrap().into()
            }
        }

        impl Display for $wrapper_name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(self.to_json_pretty().as_str(), f)
            }
        }

        impl RequestBody<$inner_type> for $wrapper_name {
            fn to_json_body(self) -> Option<ApiRequestEntity> {
                if self.0.is_none() {
                    return None;
                }
                Some(ApiRequestEntity::String(self.to_json()))
            }

            fn to_text_body(self) -> Option<ApiRequestEntity> {
                if let Some(inner) = self.0 {
                    return inner.to_text_body();
                }

                None
            }
        }

        impl RequestBody<$inner_type> for &$wrapper_name {
            fn to_json_body(self) -> Option<ApiRequestEntity> {
                if self.0.is_none() {
                    return None;
                }
                Some(ApiRequestEntity::String(self.to_json()))
            }

            fn to_text_body(self) -> Option<ApiRequestEntity> {
                if let Some(inner) = self.0.as_ref() {
                    return inner.to_text_body();
                }

                None
            }
        }
    };
}

#[doc(hidden)]
macro_rules! numeric_type_conversions {
    ($api_type:ident, $rust_type:ty, $rust_array_type:ident, $rust_array_type_option:ident, $rust_map_type_option:ident, $rust_map_type:ident, $json_func:ident) => {

        as_request_body!($api_type);

        #[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
        pub struct $rust_map_type(pub Map<$api_type>);
        as_request_body!($rust_map_type);

        impl Deref for $rust_map_type {
            type Target = Map<$api_type>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $rust_map_type {

            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl Display for $rust_map_type {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(self.to_json_pretty().as_str(), f)
            }
        }

        impl TryFrom<&JsonValue> for $rust_map_type {

            type Error = String;

            fn try_from(value: &JsonValue) -> Result<Self, String> {
                if !value.is_object() {
                    return Err("Object expected".to_string());
                }

                let mut map = LinkedHashMap::new();
                for (key, value) in value.entries() {
                    map.insert(key.to_string(), value.try_into()?);
                }

                return Ok(Self(Map(map)))
            }
        }

        impl Into<JsonValue> for &$rust_map_type {
            fn into(self) -> JsonValue {
                let mut result = JsonValue::Object(Object::new());
                for (key, value) in &self.0.0 {
                    result[key.as_str()] = value.into();
                }

                return result
            }
        }

        impl Into<JsonValue> for $rust_map_type {
            fn into(self) -> JsonValue {
                let mut result = JsonValue::Object(Object::new());
                for (key, value) in self.0.0 {
                    result[key.as_str()] = value.into();
                }

                return result
            }
        }

        option_wrapper!($rust_map_type_option, $rust_map_type);

        #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
        pub struct $rust_array_type(pub Vec<$api_type>);
        option_wrapper!($rust_array_type_option, $rust_array_type);
        as_request_body!($rust_array_type);


        impl Display for $rust_array_type {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(self.to_json_pretty().as_str(), f)
            }
        }

        impl Deref for $rust_array_type {
            type Target = Vec<$api_type>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $rust_array_type {

            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl TryFrom<&JsonValue> for $rust_array_type {
            type Error = String;

            fn try_from(value: &JsonValue) -> Result<Self, String> {
                if !value.is_array() {
                    return Err("Array expected".to_string());
                }

                let mut array = Vec::with_capacity(value.len());
                for x in value.members() {
                    array.push(x.try_into()?);
                }

                return Ok(Self(array))
            }
        }

        impl Into<JsonValue> for &$rust_array_type {
            fn into(self) -> JsonValue {
                let mut values: Vec<JsonValue> = Vec::with_capacity(self.len());
                for x in &self.0 {
                    values.push(x.into());
                }

                return JsonValue::Array(values)
            }
        }


        impl Into<JsonValue> for $rust_array_type {
            fn into(self) -> JsonValue {
                let mut values: Vec<JsonValue> = Vec::with_capacity(self.len());
                for x in self.0 {
                    values.push(x.into());
                }

                return JsonValue::Array(values)
            }
        }

        impl Deref for $api_type {
            type Target = Option<$rust_type>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $api_type {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl Display for $api_type {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                Display::fmt(self.to_json_pretty().as_str(), f)
            }
        }

        impl TryFrom<$api_type> for $rust_type {
            type Error = ();

            fn try_from(value: $api_type) -> Result<Self, Self::Error> {
                match value.0 {
                    Some(v) => Ok(v),
                    None => Err(()),
                }
            }
        }

        impl From<$rust_type> for $api_type {
            fn from(value: $rust_type) -> Self {
                Self(Some(value))
            }
        }

        impl From<Option<$rust_type>> for $api_type {
            fn from(value: Option<$rust_type>) -> Self {
                Self(value)
            }
        }

        impl TryFrom<&JsonValue> for $api_type {
            type Error = String;

            fn try_from(value: &JsonValue) -> Result<Self, String> {
                if value.is_null() {
                    return Ok(Self(None));
                }

                if let Some(num) = value.$json_func() {
                    return Ok(Self(Some(num)));
                }

                Err("Number expected".to_string())
            }
        }

        impl Into<JsonValue> for $api_type {
            fn into(self) -> JsonValue {
                match self.0 {
                    None => JsonValue::Null,
                    Some(x) => JsonValue::Number(Number::from(x)),
                }
            }
        }

        impl Into<JsonValue> for &$api_type {
            fn into(self) -> JsonValue {
                match self.0 {
                    None => JsonValue::Null,
                    Some(x) => JsonValue::Number(Number::from(x)),
                }
            }
        }

        impl PathParam<$rust_type> for $rust_type {
            fn to_path_param_simple(self) -> Option<String> {
                Some(self.to_string())
            }

            fn to_path_param_simple_explode(self) -> Option<String> {
                self.to_path_param_simple()
            }

            fn to_path_param_label(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_label_explode(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_matrix(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }

            fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }
        }

        impl PathParam<$rust_type> for &$rust_type {
            fn to_path_param_simple(self) -> Option<String> {
                Some(self.to_string())
            }

            fn to_path_param_simple_explode(self) -> Option<String> {
                self.to_path_param_simple()
            }

            fn to_path_param_label(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_label_explode(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_matrix(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }

            fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }
        }

        impl PathParam<$rust_type> for $api_type {
            fn to_path_param_simple(self) -> Option<String> {
                Some(self.0?.to_string())
            }

            fn to_path_param_simple_explode(self) -> Option<String> {
                self.to_path_param_simple()
            }

            fn to_path_param_label(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_label_explode(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_matrix(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }

            fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }
        }

        impl PathParam<$rust_type> for &$api_type {
            fn to_path_param_simple(self) -> Option<String> {
                Some(self.0?.to_string())
            }

            fn to_path_param_simple_explode(self) -> Option<String> {
                self.to_path_param_simple()
            }

            fn to_path_param_label(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_label_explode(self) -> Option<String> {
                Some(format!(".{self}"))
            }

            fn to_path_param_matrix(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }

            fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
                Some(format!(";{name}={self}"))
            }
        }
    };
}

#[doc(hidden)]
macro_rules! numeric_type {
    ($api_type:ident, $rust_type:ty, $rust_array_type:ident, $rust_array_type_option:ident, $rust_map_type_option:ident, $rust_map_type:ident, $json_func:ident) => {
        #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Default)]
        pub struct $api_type(pub Option<$rust_type>);


        numeric_type_conversions!($api_type, $rust_type, $rust_array_type, $rust_array_type_option, $rust_map_type_option, $rust_map_type, $json_func);
    }
}

#[doc(hidden)]
macro_rules! fp_numeric_type {
    ($api_type:ident, $rust_type:ty, $rust_array_type:ident, $rust_array_type_option:ident, $rust_map_type_option:ident, $rust_map_type:ident, $json_func:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        pub struct $api_type(pub Option<$rust_type>);

        impl Eq for $api_type {

        }

        //I am aware that this is dicy, but not doing this would be a big pain
        //As I suspect that for 99% of use cases this hack is irrelevant
        impl Hash for $api_type {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.clone().map(|x| x.to_bits()).hash(state);
            }
        }

        numeric_type_conversions!($api_type, $rust_type, $rust_array_type, $rust_array_type_option, $rust_map_type_option, $rust_map_type, $json_func);
    }
}

#[doc(hidden)]
macro_rules! as_request_body {
    ($typ:ty) => {
        impl RequestBody<$typ> for $typ {
            fn to_json_body(self) -> Option<ApiRequestEntity> {
                Some(ApiRequestEntity::String(self.to_json()))
            }

            fn to_text_body(self) -> Option<ApiRequestEntity> {
                None
            }
        }

        impl RequestBody<$typ> for &$typ {
            fn to_json_body(self) -> Option<ApiRequestEntity> {
                Some(ApiRequestEntity::String(self.to_json()))
            }

            fn to_text_body(self) -> Option<ApiRequestEntity> {
                None
            }
        }
    }
}

pub trait RequestBody<T: Debug> {
    fn to_json_body(self) -> Option<ApiRequestEntity>;

    fn to_text_body(self) -> Option<ApiRequestEntity>;
}


impl RequestBody<String> for String {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::String(self.clone()).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(self))
    }
}
impl RequestBody<String> for &str {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::String(self.to_string()).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(self.to_string()))
    }
}


pub trait PathParam<T: Debug> {
    fn to_path_param_simple(self) -> Option<String>;

    fn to_path_param_simple_explode(self) -> Option<String>;

    fn to_path_param_label(self) -> Option<String>;

    fn to_path_param_label_explode(self) -> Option<String>;

    fn to_path_param_matrix(self, name: &str) -> Option<String>;

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String>;
}

impl PathParam<String> for String {
    fn to_path_param_simple(self) -> Option<String> {
        Some(self)
    }
    fn to_path_param_simple_explode(self) -> Option<String> {
        Some(self)
    }

    fn to_path_param_label(self) -> Option<String> {
        Some(format!(".{self}"))
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        Some(format!(".{self}"))
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        Some(format!(";{name}={self}"))
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        Some(format!(";{name}={self}"))
    }
}

impl PathParam<String> for &str {
    fn to_path_param_simple(self) -> Option<String> {
        Some(self.to_string())
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        Some(self.to_string())
    }

    fn to_path_param_label(self) -> Option<String> {
        Some(format!(".{self}"))
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        Some(format!(".{self}"))
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        Some(format!(";{name}={self}"))
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        Some(format!(";{name}={self}"))
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
static TOKIO_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
pub fn set_tokio_runtime(runtime: tokio::runtime::Runtime) -> bool {
    TOKIO_RUNTIME.set(runtime).is_ok()
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
pub fn has_tokio_runtime() -> bool {
    TOKIO_RUNTIME.get().is_some()
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
fn get_tokio_runtime_or_error() -> io::Result<&'static tokio::runtime::Runtime> {
    match TOKIO_RUNTIME.get() {
        None => Err(io::Error::new(io::ErrorKind::Other, "Tokio runtime not initialized, call set_tokio_runtime")),
        Some(runtime) => Ok(runtime)
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
unsafe fn ffi_get_map_keys<T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display>
        (any_map: &Map<T>, debug_ref: &str) -> *mut StringArray {
    let size = any_map.len();
    let mut keys = Vec::with_capacity(size);

    for (key, _) in any_map.iter() {
        keys.push(key.as_str().into());
    }

    if size != keys.len() {
        //unlikely to happen...
        ffi_abort(format!("{}: heap corruption", debug_ref));
        unreachable!()
    }

    keys.sort();

    return Box::into_raw(Box::new(StringArray(keys)));
}

///
/// Helper function to abort the program.
/// This is only called from ffi code if the input to a ffi function is unexpected.
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
fn ffi_abort<T: Display>(why: T) {
    eprintln!("{}", why);
    eprintln!("The program has detected an illegal state and must terminate immediately!");
    std::process::abort();
}

///
/// Function pointer to an allocator to use to allocate strings and array datatypes that are returned by the ffi interface.
///
/// Param 1: size of allocation in bytes, never 0.
/// Param 2: alignment in bytes, never 0. Always a power of 2.
///
/// Returns:
/// Non-null pointer to memory capable of holding at least size bytes and aligned to the given alignment.
/// The returned memory does not need to be initialized to any particular value by the allocator.
///
/// Info:
/// All functions that take an allocator as a parameter tend to return the allocated pointer directly to the caller.
/// It is expected that the allocated data is freed by the caller using means that are associated with the allocator.
/// After the execution returns back to the caller rust does NOT keep a reference to the allocator or any allocation that was made.
///
/// Program will be aborted if:
/// - Null is returned.
/// - The returned pointer is misaligned.
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type FFIAllocator = Option<extern "C" fn(usize, usize) -> *mut c_void>;

///
/// Utility function to invoke the FFIAllocator and ask for memory.
/// This function does some sanity checks around the call to the allocator. Such as checking
/// that we do not (for some reason) request 0 bytes or an illegal alignment.
/// It also checks that the allocator actually respected our desired alignment.
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
unsafe fn ffi_alloc(allocator: FFIAllocator, size: usize, alignment: usize) -> *mut c_void {
    if size == 0 || alignment == 0 {
        ffi_abort("ffi_alloc bad parameter");
        unreachable!()
    }

    if (alignment - 1) & alignment != 0 {
        ffi_abort("ffi_alloc bad parameter, alignment not power of 2, this is a bug on the rust side");
        unreachable!()
    }

    match allocator {
        None => {
            ffi_abort("ffi_alloc null allocator");
            unreachable!()
        }
        Some(alloc) => {
            let allocation = alloc(size, alignment);
            if allocation.is_null() {
                ffi_abort("ffi_alloc allocator returned null");
                unreachable!()
            }

            if allocation as usize & (alignment-1) != 0 {
                ffi_abort("ffi_alloc allocator returned misaligned pointer");
                unreachable!()
            }

            allocation
        }
    }
}

///
/// Stream that is implemented on the other side of the ffi boundary.
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct FFIStream {
    func: *const c_void,
    destructor: *const c_void,
    opaque: *mut c_void,
}

/// We assume that all function pointers that implement the stream can be called in whatever thread meaning they are Send.
/// But it would not make sense to expect concurrent calls to them to work, so they are not Sync.
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
unsafe impl Send for FFIStream {}

///Read impl that calls out to ffi to read some bytes from somewhere
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
impl io::Read for FFIStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut size = buf.len();
        if size == 0 {
            return Ok(0)
        }
        unsafe {
            let read_func : extern "C" fn(*mut c_void, *mut u8, *mut usize) -> u32 =  mem::transmute_copy(&self.func);
            let result = read_func(self.opaque, buf.as_mut_ptr(), &mut size);
            if result == 0 {
                if size > buf.len() {
                    //Buffer overflow probably happened :( make valiant effort to crash the program.
                    //if buf is stack allocated then there is no way to know if we actually get here.
                    //We have no way to influence if buf is stack allocated or not.
                    //Even if buf is heap allocated then depending on what was behind the allocation we may not get here either.
                    //there is also no way to tell if ffi_abort even performs its job properly in this corrupted state.
                    //the only guarantee we can give is that if no buffer overflow
                    //actually happened on the other side of ffi and only the value
                    //written to size was garbage then we will abort.
                    ffi_abort("FFIStream read caused buffer overflow");
                    unreachable!()
                }

                return Ok(size);
            }

            //TODO this is not great and should be improved.
            Err(io::Error::new(io::ErrorKind::Other, format!("FFIStream Err {}", result)))
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
impl Drop for FFIStream {
    fn drop(&mut self) {
        if self.destructor.is_null() {
            return;
        }

        unsafe {
            let destructor_func : extern "C" fn(*mut c_void) =  mem::transmute_copy(&self.destructor);
            destructor_func(self.opaque);
        }
    }
}

///
/// This type represents a binary stream of Data that my or may not be present.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct OStream(pub Option<Stream>);

impl Deref for OStream {
    type Target = Option<Stream>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for OStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.is_none() {
            f.write_str("NoStream")
        } else {
            f.write_str("Stream")
        }
    }
}


#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl TryInto<Box<dyn io::Read+Send>> for OStream {
    type Error = either::Either<OStream, PoisonError<Box<dyn io::Read+Send>>>;

    fn try_into(self) -> Result<Box<dyn io::Read + Send>, Self::Error> {
        self.0.ok_or(either::Either::Left(OStream(None)))?
            .try_into()
            .map_err(|e| match e {
                either::Either::Left(x) => either::Either::Left(OStream(Some(x))),
                either::Either::Right(x) => either::Either::Right(x),
            })
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl TryInto<Box<dyn tokio::io::AsyncRead+Send+Unpin>> for OStream {
    type Error = either::Either<OStream, PoisonError<Box<dyn tokio::io::AsyncRead+Send+Unpin>>>;

    fn try_into(self) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, Self::Error> {
        self.0.ok_or(either::Either::Left(OStream(None)))?
            .try_into()
            .map_err(|e| match e {
                either::Either::Left(x) => either::Either::Left(OStream(Some(x))),
                either::Either::Right(x) => either::Either::Right(x),
            })
    }
}

#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl From<Box<dyn io::Read+Send+Unpin>> for OStream {
    fn from(value: Box<dyn io::Read+Send+Unpin>) -> Self {
        OStream(Some(Stream::Blocking(BlockingStream{
            reader: Arc::new(Mutex::new(value)),
            #[cfg(feature = "async")]
            data: Arc::new(Mutex::new(None)),
            #[cfg(feature = "async")]
            current_data: None,
            #[cfg(feature = "async")]
            current_pos: 0,
        })))
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl From<Box<dyn tokio::io::AsyncRead+Send +Unpin>> for OStream {
    fn from(value: Box<dyn tokio::io::AsyncRead+Send+Unpin>) -> Self {
        OStream(Some(Stream::Async(AsyncStream(Arc::new(Mutex::new(value))))))
    }
}

impl From<Stream> for OStream {
    fn from(value: Stream) -> Self {
        OStream(Some(value))
    }
}

///
/// This type represents reference counted Stream of binary data.
/// It is for example trivially possible to turn a 'File' (or any Read+Send impl) into this type.
///
/// This type is Send, all Read operations are synchronized,
/// however it is very unlikely tha reading from multiple threads concurrently makes sense.
/// The synchronization mechanisms just ensure that the underlying Read impl is not called concurrently.
///
/// Please note that this Struct implements both io::Read for use in "sync/blocking Rust" and AsyncRead.
/// Using both blocking and async read on the same stream may yield unexpected/corrupted data.
/// Please stick to using one or the other for each Stream.
///
///
///
#[derive(Clone)]
pub enum Stream {
    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    Blocking(BlockingStream),
    #[cfg(feature = "async")]
    #[cfg(not(target_arch = "wasm32"))]
    Async(AsyncStream),
    Vec(VecStream)
}

#[derive(Clone)]
pub struct VecStream(Arc<Mutex<VecStreamInner>>);

struct VecStreamInner {
    data: Vec<u8>,
    position: usize
}

impl VecStream {
    pub fn new(data: Vec<u8>) -> VecStream {
        VecStream(Arc::new(Mutex::new(VecStreamInner {
            data,
            position: 0
        })))
    }

    pub fn remaining_data(&self) -> Vec<u8> {
        let mut g = self.0.lock().unwrap();
        let sl = g.data.as_slice();
        let to_copy = sl.len()-g.position;
        if to_copy == 0 {
            return Vec::new()
        }

        let mut vec = Vec::with_capacity(to_copy);
        vec.as_mut_slice()[0..to_copy].copy_from_slice(&sl[g.position..g.position+to_copy]);
        g.position = sl.len();
        vec
    }

    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    pub async fn next_chunk(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut g = self.0.lock().unwrap();
        let sl = g.data.as_slice();
        let to_copy = min(sl.len()-g.position, buf.len());
        if to_copy == 0 {
            return Ok(0)
        }

        buf[0..to_copy].copy_from_slice(&sl[g.position..g.position+to_copy]);
        g.position += to_copy;
        Ok(to_copy)
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl tokio::io::AsyncRead for VecStream {
    fn poll_read(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        let mut g = self.0.lock().unwrap();
        let sl = g.data.as_slice();
        let to_copy = min(sl.len()-g.position, buf.remaining());
        if to_copy == 0 {
            return std::task::Poll::Ready(Ok(()))
        }

        tokio_util::bytes::BufMut::put(buf, &g.data.as_slice() [g.position..g.position+to_copy]);
        g.position += to_copy;
        std::task::Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl io::Read for VecStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut g = self.0.lock().unwrap();
        let sl = g.data.as_slice();
        let to_copy = min(sl.len()-g.position, buf.len());
        if to_copy == 0 {
            return Ok(0)
        }

        buf[0..to_copy].copy_from_slice(&sl[g.position..g.position+to_copy]);
        g.position += to_copy;
        Ok(to_copy)
    }
}


impl Stream {
    #[cfg(feature = "async")]
    pub async fn next_chunk(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(blocking) => {
                tokio::io::AsyncReadExt::read(blocking, buf).await
            }
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(s_async) => {
                tokio::io::AsyncReadExt::read(s_async, buf).await
            }
            Stream::Vec(v) => v.next_chunk(buf).await
        }
    }

    pub fn remaining_data(&self) -> io::Result<Vec<u8>> {
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(s_block) => {
                let mut data = Vec::new();
                io::Read::read_to_end(&mut s_block.clone(), &mut data)?;
                Ok(data)
            }
            #[cfg(feature = "async")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(s_async) => {
                get_tokio_runtime_or_error()?.block_on(async move {
                    let mut data = Vec::new();
                    let r = tokio::io::AsyncReadExt::read_to_end(&mut s_async.clone(), &mut data).await;
                    if r.is_err() {
                        Err(r.unwrap_err())
                    } else {
                        Ok(data)
                    }
                })

            }
            Stream::Vec(v) => Ok(v.remaining_data())
        }
    }
}



/// Represents a Stream on a Blocking io::Read implementation.
/// This type contains the necessary synchronization glue to allow for use as an AsyncRead if required.
///
#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct BlockingStream {
    pub reader: Arc<Mutex<Box<dyn io::Read+Send>>>,
    #[cfg(feature = "async")]
    data: Arc<Mutex<Option<io::Result<Vec<u8>>>>>,
    #[cfg(feature = "async")]
    current_data: Option<Vec<u8>>,
    #[cfg(feature = "async")]
    current_pos: usize,
}

#[cfg(feature = "blocking")]
#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl tokio::io::AsyncRead for BlockingStream {
    fn poll_read(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        if self.current_data.is_some() {
            let cdata = self.current_data.as_ref().unwrap();
            let remaining = cdata.len() - self.current_pos;
            let to_copy = min(remaining, buf.remaining());
            tokio_util::bytes::BufMut::put(buf, &cdata.as_slice()[self.current_pos..self.current_pos+to_copy]);

            if to_copy != remaining {
                self.current_pos += to_copy;
            } else {
                self.current_data = None;
                self.current_pos = 0;
            }
            return std::task::Poll::Ready(Ok(()));
        }

        let mut guard = self.data.lock().expect("Reader panicked");

        if guard.is_none() {
            let data_ref = self.data.clone();
            let reader_ref = self.reader.clone();
            let waker = cx.waker().clone();

            get_tokio_runtime_or_error()?.spawn_blocking(move || {
                let data_guard = data_ref.lock();
                if data_guard.is_err() {
                    waker.wake();
                    return;
                }
                let mut data_guard = data_guard.unwrap();
                let reader_guard = reader_ref.lock();
                if reader_guard.is_err() {
                    waker.wake();
                    panic!("reader_guard is poisoned");
                }

                let mut buffer = vec![0; 0x1_00_00];
                let result = reader_guard.unwrap().read(buffer.as_mut_slice());
                if result.is_err() {
                    *data_guard = Some(Err(result.unwrap_err()));
                    waker.wake();
                    return;
                }
                buffer.truncate(result.unwrap());
                *data_guard = Some(Ok(buffer));
                waker.wake();
            });
            return std::task::Poll::Pending;
        }

        let data_result = mem::replace(&mut *guard, None).unwrap();

        if data_result.is_err() {
            return std::task::Poll::Ready(data_result.map(|_| {}))
        }

        drop(guard);

        let data = data_result.unwrap();

        let to_copy = min(data.len(), buf.remaining());
        tokio_util::bytes::BufMut::put(buf, &data.as_slice()[0..to_copy]);
        if to_copy != data.len() {
            self.current_data = Some(data);
            self.current_pos = to_copy;
        } else {
            self.current_data = None;
            self.current_pos = 0;
        }

        std::task::Poll::Ready(Ok(()))
    }
}


#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl io::Read for BlockingStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Mutex Poisoned"))?
            .read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        self.reader.lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Mutex Poisoned"))?
            .read_vectored(bufs)
    }
    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.reader.lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Mutex Poisoned"))?
            .read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.reader.lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Mutex Poisoned"))?
            .read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.reader.lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Mutex Poisoned"))?
            .read_exact(buf)
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct AsyncReqwestResponseStream(Arc<Mutex<AsyncReqwestResponseStreamInner>>);

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
struct AsyncReqwestResponseStreamInner {
    response: Option<reqwest::Response>,
    eof: bool,
    current_data: Option<io::Result<Vec<u8>>>,
    current_index: usize,
}


#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl tokio::io::AsyncRead for AsyncReqwestResponseStream {
    fn poll_read(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        let mut guard = self.0.lock().unwrap();
        if guard.eof {
            return std::task::Poll::Ready(Ok(()))
        }

        if guard.current_data.is_some() {
            let result_ref = guard.current_data.as_ref().unwrap();
            if result_ref.is_err() {
                let moved_error = std::mem::replace(&mut guard.current_data, None);
                return std::task::Poll::Ready(Err(moved_error.unwrap().unwrap_err()));
            }

            let sl = result_ref.as_ref().unwrap().as_slice();
            let size = sl.len();
            let to_copy = min(sl.len() - guard.current_index, buf.remaining());
            tokio_util::bytes::BufMut::put(buf, &sl[guard.current_index..guard.current_index+to_copy]);
            guard.current_index+=to_copy;
            if guard.current_index >= size {
                guard.current_data = None;
                guard.current_index = 0;
            }
            return std::task::Poll::Ready(Ok(()))
        }

        let waker = cx.waker().clone();
        let arc_ref = self.0.clone();
        drop(guard);
        tokio::spawn(async move {
            let dta;
            {
                let mut guard = arc_ref.lock().unwrap();
                if guard.current_data.is_some() || guard.eof {
                    waker.wake();
                    return;
                }

                dta = std::mem::replace(&mut guard.response, None); //We take ownership of the response for a bit...
                if dta.is_none() {
                    //Other thread currently in .await, and we cannot hold the mutex guard while we are in await...
                    waker.wake();
                    return;
                }
            }

            let mut response = dta.unwrap();
            let next_element = response.chunk().await;
            let mut guard = arc_ref.lock().unwrap();
            guard.response = Some(response);

            if next_element.is_err() {
                guard.current_data = Some(Err(std::io::Error::new(std::io::ErrorKind::Other, next_element.unwrap_err())));
                waker.wake();
                return;

            }

            let next_element = next_element.unwrap();
            if next_element.is_none() {
                guard.eof = true;
                waker.wake();
                return;
            }

            guard.current_data = Some(Ok(next_element.unwrap().to_vec()));
            guard.current_index = 0;
        });
        std::task::Poll::Pending
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl From<reqwest::Response> for Stream {
    fn from(value: reqwest::Response) -> Self {
        let stream = AsyncReqwestResponseStream(
            Arc::new(Mutex::new(AsyncReqwestResponseStreamInner {
                response: Some(value),
                eof: false,
                current_data: None,
                current_index: 0,
            }))
        );

        //TODO get rid of this double mutex somehow later...
        Stream::Async(AsyncStream(Arc::new(Mutex::new(Box::new(stream)))))
    }
}

/// Stream impl for a AsyncRead.
/// This Struct also provides a poor man's implementation of io::Read for AsyncRead.
#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct AsyncStream(pub Arc<Mutex<Box<dyn tokio::io::AsyncRead+Send+Unpin>>>);

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl tokio::io::AsyncRead for AsyncStream {
    fn poll_read(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        let guard = self.0.lock();
        if guard.is_err() {
            return std::task::Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Mutex Poisoned")));
        }

        let mut g = guard.unwrap();
        let mut pinned = std::pin::pin!(&mut *g);
        pinned.as_mut().poll_read(cx, buf)
    }
}

#[cfg(feature = "async")]
#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl io::Read for AsyncStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        get_tokio_runtime_or_error()?
            .block_on(async {
                tokio::io::AsyncReadExt::read(self, buf).await
            })
    }
}

/// Better than nothing
impl Display for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stream")
    }
}

impl Stream {
    pub fn strong_count(&self) -> usize {
        #[allow(unreachable_patterns)]
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(s) => Arc::strong_count(&s.reader),
            #[cfg(feature = "async")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(s) => Arc::strong_count(&s.0),
            Stream::Vec(s) => Arc::strong_count(&s.0),
            _=> 0,
        }

    }
}

#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl From<Box<dyn io::Read+Send>> for Stream {
    fn from(value: Box<dyn io::Read + Send>) -> Self {
        Stream::Blocking(BlockingStream {
            reader: Arc::new(Mutex::new(value)),
            #[cfg(feature = "async")]
            data: Arc::new(Mutex::new(None)),
            #[cfg(feature = "async")]
            current_data: None,
            #[cfg(feature = "async")]
            current_pos: 0,
        })
    }
}

impl From<Vec<u8>> for Stream {
    fn from(value: Vec<u8>) -> Self {
        Stream::Vec(VecStream::new(value))
    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl From<Box<dyn tokio::io::AsyncRead+Send+Unpin>> for Stream {
    fn from(value: Box<dyn tokio::io::AsyncRead+Send+Unpin>) -> Self {
        Stream::Async(AsyncStream( Arc::new(Mutex::new(value))))
    }
}


#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl tokio::io::AsyncRead for Stream {
    fn poll_read(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> std::task::Poll<io::Result<()>> {
        match std::pin::Pin::get_mut(self) {
            Stream::Async(s) => std::pin::pin!(s).poll_read(cx, buf),
            #[cfg(feature = "blocking")]
            Stream::Blocking(s) => std::pin::pin!(s).poll_read(cx, buf),
            Stream::Vec(s) => std::pin::pin!(s).poll_read(cx, buf),
        }
    }
}

#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl io::Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Blocking(a) => std::io::Read::read(a, buf),
            #[cfg(feature = "async")]
            Stream::Async(a) => std::io::Read::read(a, buf),
            Stream::Vec(v) => std::io::Read::read(v, buf),
        }
    }
}


///
/// Try to get the Box<dyn Read+Send> out of the Stream.
/// This succeeds if there is only 1 reference to this Box.
///
/// It will fail if there is more than 1 reference to this Stream or if the Stream is not a BlockingStream.
///
/// On failure this will give you the stream back.
/// In the case that a previous call to read() did panic inside the read function
/// of the Box a PoisonError<Box<dyn Read+Send>> is returned to you instead.
///
#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
impl TryInto<Box<dyn io::Read+Send>> for Stream {
    type Error = either::Either<Stream, PoisonError<Box<dyn io::Read+Send>>>;

    fn try_into(self) -> Result<Box<dyn io::Read + Send>, Self::Error> {
        #[allow(unreachable_patterns)]
        match self {
            Stream::Blocking(bs) => {
                Arc::try_unwrap(bs.reader)
                    .map_err(|e| either::Either::Left(Stream::Blocking(BlockingStream {
                        reader: e,
                        #[cfg(feature = "async")]
                        data: Arc::new(Mutex::new(None)),
                        #[cfg(feature = "async")]
                        current_data: None,
                        #[cfg(feature = "async")]
                        current_pos: 0,
                    })))?
                    .into_inner()
                    .map_err(|e| either::Either::Right(e))
            }
            s => Err(either::Either::Left(s))
        }

    }
}

#[cfg(feature = "async")]
#[cfg(not(target_arch = "wasm32"))]
impl TryInto<Box<dyn tokio::io::AsyncRead+Send+Unpin>> for Stream {
    type Error = either::Either<Stream, PoisonError<Box<dyn tokio::io::AsyncRead+Send+Unpin>>>;

    fn try_into(self) -> Result<Box<dyn tokio::io::AsyncRead+Send+Unpin>, Self::Error> {
        #[allow(unreachable_patterns)]
        match self {
            Stream::Async(bs) => {
                Arc::try_unwrap(bs.0)
                    .map_err(|e| either::Either::Left(Stream::Async(AsyncStream(e))))?
                    .into_inner()
                    .map_err(|e| either::Either::Right(e))
            }
            s => Err(either::Either::Left(s))
        }

    }
}

/// Pointer based implementation of Hash.
impl Hash for Stream {
    fn hash<H: Hasher>(&self, state: &mut H) {
        #[allow(unreachable_patterns)]
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(s) => Arc::as_ptr(&s.reader).hash(state),
            #[cfg(feature = "async")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(s) => Arc::as_ptr(&s.0).hash(state),
            Stream::Vec(s) => Arc::as_ptr(&s.0).hash(state),
            _=> state.write_u8(0u8)
        }
    }
}
impl PartialEq<Self> for Stream {
    fn eq(&self, other: &Self) -> bool {
        #[allow(unreachable_patterns)]
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(b) => match other {
                Stream::Blocking(o) => Arc::ptr_eq(&b.reader, &o.reader),
                _=> false,
            }
            #[cfg(feature = "async")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(a) => match other {
                Stream::Async(o) => Arc::ptr_eq(&a.0, &o.0),
                _=> false,
            }
            Stream::Vec(a) => match other {
                Stream::Vec(o) => Arc::ptr_eq(&a.0, &o.0),
                _=> false,
            }
            _=> false
        }

    }
}

impl Eq for Stream {

}

impl Default for Stream {
    fn default() -> Self {
        Stream::Vec(VecStream::new(Vec::new()))
    }
}

impl Debug for Stream {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        #[allow(unreachable_patterns)]
        match self {
            #[cfg(feature = "blocking")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Blocking(b) => f.write_str(format!("StreamBlocking[{}]", Arc::strong_count(&b.reader)).as_str()),
            #[cfg(feature = "async")]
            #[cfg(not(target_arch = "wasm32"))]
            Stream::Async(a) => f.write_str(format!("StreamAsync[{}]", Arc::strong_count(&a.0)).as_str()),
            Stream::Vec(a) => f.write_str(format!("StreamVec[{}]", Arc::strong_count(&a.0)).as_str()),
            _=> f.write_str("Stream[?]")
        }
    }
}


/// This function creates a reference counted Stream of binary data that can be used to provide complex request bodies over
/// the ffi boundary.
///
/// param state:
///   opaque state pointer that is passed through to other function calls, can be null if the stream requires no state.
///   any function that mutates the state must ensure that such a mutation becomes visible to all other threads of the program
///   before the function returns if that is required for consistency of the state.
///   rust does not dereference or interact with this pointer.
///
/// param reader:
///   function pointer, non-null.
///   The rust function signature is: extern "C" fn(*mut c_void, *mut u8, *mut usize) -> u32.
///   The C function signature is: extern "C" uint32_t func(void* state, *uint8_t buffer, uintptr_t* length)
///   The function has 3 parameters.
///   param1 is the 'state' param. param2 is a non-null pointer to a buffer. param3 is a non-null pointer to the length of the buffer.
///
///   The function is expected to read the length from the provided pointer and place up to that amount of data into the buffer.
///   The initial value of the length pointer is guaranteed to be at least 1.
///   Upon completion the function is expected to write the amount of data actually read into the length pointer.
///   It is perfectly valid to read less bytes than requested.
///   The return value of the function must be 0 to indicate that the operation was successful.
///
///   Writing a value of 0 into the length pointer and returning 0 is used to signal end of File (EOF).
///   The function should NOT assume that it is never called again after yielding EOF once.
///   It is unlikely that it is called again after signaling EOF once.
///   If it is called again then it can (and should) again yield EOF.
///
///   Any non-zero value will be treated as an error and such an error will be propagated
///   to whichever internal logic caused the stream to be read.
///   The function can expect the numeric error value to be present in the error message/info in some way.
///   The meaning behind the numeric error values is up to the implementation.
///   In case of error the values written to the buffer or length pointer are ignored.
///   The function should NOT assume that it is never called again after yielding an Error once.
///   It is unlikely that it is called again after signaling an Error once.
///   If it is called again then it can (and should) yield Error again.
///
///   The function is guaranteed to never be invoked concurrently by two or more threads at once,
///   but it should NOT make any assumption about which thread it is called in.
///   Successive invocation of the function may be from different threads.
///
///   This function is guaranteed to never be called concurrently with the destructor function.
///   This function is guaranteed to never be called again as soon as the destructor function is called.
///
///   Note: The program will attempt to abort if a value larger than the initial value is written to the length pointer.
///   However since this may be caused by a buffer overflow (meaning this amount of data was actually written to the buffer)
///   and the buffer may have been stack allocated then there is no guarantee that the execution flow even returns to the rust code or that
///   the abort routines function properly. If no buffer overflow actually occurred then an abort is guaranteed.
///
/// param destructor:
///   function pointer, can be null.
///   The rust function signature is: extern "C" fn(*mut c_void)
///   The C function signature is: extern "C" void func(void* state)
///
///   This function (if non-null) is guaranteed to be called exactly once after the reference count of the created Stream reaches 0.
///   This is most likely the case when stream_free is called. In some circumstances a reference to the Stream may be contained
///   in an ApiError or Response object. Should that be the case then the destructor is invoked once the ApiError or Response object is freed.
///
///   The function should not make any assumption about which thread it is called in.
///   This function is guaranteed to never be called concurrently with the reader function.
///
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn stream_new(reader: *const c_void, destructor: *const c_void, state: *mut c_void) -> *mut Stream {
    if reader.is_null() {
        ffi_abort("stream_new called with null pointer reader function");
        unreachable!()
    }
    Box::into_raw(Box::new(Stream::from(Box::new(FFIStream {
        func: reader,
        destructor,
        opaque: state,
    }) as Box<dyn io::Read+Send>)))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn stream_free(inst: *mut Stream) {
    if inst.is_null() {
        ffi_abort("stream_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

///
/// This function reads bytes from a stream object.
/// param "inst" holds a pointer to a stream object
/// param "buf" holds a pointer to a buffer
/// param "size" holds a pointer to the maximum amount of bytes to be read.
///
/// On Success: 0 is returned.
/// Then the amount of bytes read into buf is written into the "size" pointer.
/// The amount of bytes read is always positive unless the buffer size was initially set to 0 or end of stream was reached.
///
/// On Failure: a positive integer is returned indicating the type of error.
/// TODO table of possible return value here, maybe enum?
///
/// Aborts program if:
/// inst is null
/// buf is null
/// size is null
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn stream_read(inst: *mut Stream, buf: *mut u8, size: *mut usize) -> u32 {
    if buf.is_null() {
        ffi_abort("stream_read called with buf null pointer!");
        unreachable!()
    }

    if size.is_null() {
        ffi_abort("stream_read called with size null pointer!");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("stream_read was called with a null instance pointer");
            unreachable!()
        }
        Some(stream) => {
            let buffer_size = *size;
            if buffer_size == 0 {
                return 0;
            }

            let sl = std::slice::from_raw_parts_mut(buf, buffer_size);

            match io::Read::read(stream, sl) {
                Ok(len) => {
                    *size = len;
                    return 0;
                }
                Err(err) => {
                    //This is better than nothing
                    match err.kind() {
                        io::ErrorKind::NotFound => 1,
                        io::ErrorKind::PermissionDenied => 2,
                        io::ErrorKind::ConnectionRefused => 3,
                        io::ErrorKind::ConnectionReset => 4,
                        io::ErrorKind::ConnectionAborted => 5,
                        io::ErrorKind::NotConnected => 6,
                        io::ErrorKind::AddrInUse => 7,
                        io::ErrorKind::AddrNotAvailable => 8,
                        io::ErrorKind::BrokenPipe => 9,
                        io::ErrorKind::AlreadyExists => 10,
                        io::ErrorKind::WouldBlock => 11,
                        io::ErrorKind::InvalidInput => 12,
                        io::ErrorKind::InvalidData => 13,
                        io::ErrorKind::TimedOut => 14,
                        io::ErrorKind::WriteZero => 15,
                        io::ErrorKind::Interrupted => 16,
                        io::ErrorKind::Unsupported => 17,
                        io::ErrorKind::UnexpectedEof => 18,
                        io::ErrorKind::OutOfMemory => 19,
                        io::ErrorKind::Other => 20,
                        _ => 21,
                    }
                }
            }
        }
    }
}



#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct StringMap(pub Map<OString>);
option_wrapper!(OStringMap, StringMap);
as_request_body!(StringMap);


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_new() -> *mut StringMap {
    Box::into_raw(Box::new(StringMap::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_free(inst: *mut StringMap) {
    if inst.is_null() {
        ffi_abort("string_map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_keys(inst: *const StringMap) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("string_map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "StringMap_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_remove(inst: *mut StringMap, key: *const c_char) {
    if key.is_null() {
        ffi_abort("StringMap_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("StringMap_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("StringMap_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("StringMap_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_set(inst: *mut StringMap, key: *const c_char, value: *const c_char) {
    if key.is_null() {
        ffi_abort("StringMap_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("StringMap_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    let the_new_value = match value.is_null() {
        false => {
            let value = CStr::from_ptr(value).to_str();
            if value.is_err() {
                ffi_abort("StringMap_set called with value that is not valid 0 terminated utf-8. Pointer my be invalid?");
                unreachable!()
            }

            value.unwrap().into()
        }
        true => OString::default(),
    };

    match inst.as_mut() {
        None => {
            ffi_abort("StringMap_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.insert(key.to_string(), the_new_value).is_some() {
                ffi_abort(format!("StringMap_set was called with key {} that already exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringMap_get(inst: *const StringMap, key: *const c_char, buffer: *mut c_char, len: *mut usize) -> bool {
    if key.is_null() {
        ffi_abort("StringMap_get was called with a key null pointer");
        unreachable!()
    }

    if len.is_null() {
        ffi_abort("StringMap_get was called with a len null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("StringMap_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("StringMap_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key).map(|a| a.clone());
            if value.is_none() {
                ffi_abort(format!("StringMap_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                len.write_unaligned(0);
                return true;
            }

            let bytes = value.as_ref().unwrap().as_bytes();
            if len.read_unaligned() < bytes.len()+1 {
                len.write_unaligned(bytes.len()+1);
                return false;
            }
            len.write_unaligned(bytes.len()+1);
            if buffer.is_null() {
                return true;
            }
            for (idx, ele) in bytes.iter().enumerate() {
                match *ele as c_char {
                    0 => buffer.wrapping_add(idx).write_unaligned(32),
                    e => buffer.wrapping_add(idx).write_unaligned(e)
                }
            }
            buffer.wrapping_add(bytes.len()).write_unaligned(0);
            true
        }
    }
}


impl Deref for StringMap {
    type Target = Map<OString>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StringMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<&JsonValue> for StringMap {

    type Error = String;

    fn try_from(value: &JsonValue) -> Result<Self, String> {
        Ok(Self(value.try_into()?))
    }
}

impl Into<JsonValue> for &StringMap {
    fn into(self) -> JsonValue {
        (&self.0).into()
    }
}


impl Into<JsonValue> for StringMap {
    fn into(self) -> JsonValue {
        self.0.into()
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BoolArray(pub Vec<OBool>);
option_wrapper!(OBoolArray, BoolArray);
as_request_body!(BoolArray);


impl Deref for BoolArray {
    type Target = Vec<OBool>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BoolArray {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<&JsonValue> for BoolArray {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        match value {
            JsonValue::Array(ar) => {
                Ok(BoolArray(ar.iter().map(|a| OBool(a.as_bool())).collect()))
            }
            _=> Err("Expected Boolean Array".to_string()),
        }
    }
}

impl Into<JsonValue> for &BoolArray {
    fn into(self) -> JsonValue {
        JsonValue::Array(self.0.iter().map(|a| a.into()).collect())
    }
}


impl Into<JsonValue> for BoolArray {
    fn into(self) -> JsonValue {
        self.0.into()
    }
}


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_new() -> *mut BoolArray {
    Box::into_raw(Box::new(BoolArray::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_free(inst: *mut BoolArray) {
    if inst.is_null() {
        ffi_abort("BoolArray_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_size(inst: *mut BoolArray) -> usize {
    if inst.is_null() {
        ffi_abort("BoolArray_size free(NULL)");
        unreachable!()
    }
    match inst.as_mut() {
        None => {
            ffi_abort("BoolArray_size was called with a null instance pointer");
            unreachable!()
        }
        Some(vec) => vec.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_add(inst: *mut BoolArray, value: *const bool) {
    match inst.as_mut() {
        None => {
            ffi_abort("BoolArray_add was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            vec.push(match value.as_ref() {
                None => OBool::default(),
                Some(b) => OBool::from(b)
            });
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_remove(inst: *mut BoolArray, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("BoolArray_remove was called with a null instance pointer");
            unreachable!()
        }
        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("BoolArray_remove index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            vec.remove(idx);
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_set(inst: *mut BoolArray, idx: usize, value: *const bool) {
    match inst.as_mut() {
        None => {
            ffi_abort("BoolArray_set was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("BoolArray_set index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            vec[idx] = match value.as_ref() {
                None => OBool::default(),
                Some(b) => OBool::from(b),
            };
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn BoolArray_get(inst: *const BoolArray, idx: usize, is_null: *mut bool) -> bool {
    match inst.as_ref() {
        None => {
            ffi_abort("BoolArray_get was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("BoolArray_get index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            match vec[idx].as_ref() {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }
                    false
                }

                Some(bool) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    *bool
                }
            }
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct StringArray(pub Vec<OString>);
option_wrapper!(OStringArray, StringArray);
as_request_body!(StringArray);

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_new() -> *mut StringArray {
    Box::into_raw(Box::new(StringArray::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_free(inst: *mut StringArray) {
    if inst.is_null() {
        ffi_abort("StringArray_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_size(inst: *mut StringArray) -> usize {
    if inst.is_null() {
        ffi_abort("StringArray_free free(NULL)");
        unreachable!()
    }
    match inst.as_mut() {
        None => {
            ffi_abort("StringArray_size was called with a null instance pointer");
            unreachable!()
        }
        Some(vec) => vec.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_add(inst: *mut StringArray, string: *const c_char) {
    if string.is_null() {
        ffi_abort("StringArray_add was called with a null string pointer");
        unreachable!()
    }

    let string = CStr::from_ptr(string).to_str();
    if string.is_err() {
        ffi_abort("called StringArray_add with string that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let string = OString::from(string.unwrap());

    match inst.as_mut() {
        None => {
            ffi_abort("StringArray_add was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            vec.push(string);
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_remove(inst: *mut StringArray, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("StringArray_remove was called with a null instance pointer");
            unreachable!()
        }
        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("StringArray_remove index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            vec.remove(idx);
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_set(inst: *mut StringArray, idx: usize, string: *const c_char) {
    if string.is_null() {
        ffi_abort("StringArray_set was called with a null string pointer");
        unreachable!()
    }

    let string = CStr::from_ptr(string).to_str();
    if string.is_err() {
        ffi_abort("called StringArray_set with string that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let string = OString::from(string.unwrap());

    match inst.as_mut() {
        None => {
            ffi_abort("StringArray_set was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("StringArray_set index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            vec[idx] = string;
        }
    }
}


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub(crate) struct StringArrayCopy {
    ///Amount of entries in the strings pointer array.
    pub count: usize,
    ///Pointer array with all the strings
    ///Contains NULL for null Strings.
    pub strings: *mut *const c_char,

    ///Opaque data of unknown size
    opaque_data: [usize; 0]
}

///
/// This function copies parts of the string array into a multiple memory allocations of the allocator.
///
/// Returns:
/// never return null.
///
/// Info:
/// The resulting pointer is allocated directly by the allocator and must be freed by the caller.
/// In addition, every pointer that is NON NULL inside the strings member of the returned struct
/// is also allocated by the allocator and must be freed by the caller.
///
/// Info:
/// All strings are 0 terminated utf-8.
/// Should the strings themselves contain any 0 bytes then those
/// bytes are replaced by a '32' (whitespace) byte.
///
/// Info:
/// NULL elements inside the array are represented as null pointers in the strings member.
///
/// Info:
/// The layout/size of data in opaque_data is an implementation detail.
///
/// Aborts program if:
/// - inst is null
/// - len is 0
/// - the last element to be copied is out of bounds
/// - idx+len overflows
/// - allocator is null
/// - allocator returns null
/// - allocator returns misaligned pointer
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_copy(inst: *const StringArray, idx: usize, len: usize, allocator: FFIAllocator) -> *mut StringArrayCopy {
    if len == 0 {
        ffi_abort("StringArray_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("StringArray_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("StringArray_copy was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if needed_length > vec.len() {
                ffi_abort(format!("StringArray_copy index {} out of bounds for array size {}", needed_length-1, vec.len()));
                unreachable!()
            }

            let total_size = std::mem::size_of::<StringArrayCopy>() +
                ((len+1) * size_of::<*mut c_char>());

            let alloc = ffi_alloc(allocator,
                                  total_size,
                                  std::mem::align_of::<StringArrayCopy>());


            let struct_ptr : *mut StringArrayCopy = alloc.cast();
            //I cannot use the offset_of macro because cbindgen dies because of it for some reason
            let opaque_data_offset = ((*struct_ptr).opaque_data.as_ptr() as usize) - struct_ptr as usize;

            let struct_ref = struct_ptr.as_mut().unwrap();
            struct_ref.count = len;
            struct_ref.strings = alloc.add(opaque_data_offset).cast();

            let string_ptr_slice = std::slice::from_raw_parts_mut(struct_ref.strings, len+1);

            for i in 0..len {
                match vec[idx+i].as_ref() {
                    None => {
                        string_ptr_slice[i] = std::ptr::null_mut();
                    }
                    Some(data) => {
                        let bytes = data.as_bytes();
                        let allocation = ffi_alloc(allocator, bytes.len()+1, 1).cast();
                        let mut data_index = 0;
                        string_ptr_slice[i] = allocation;
                        for ele in data.as_bytes() {
                            match *ele as c_char {
                                0 => allocation.add(data_index).write_unaligned(32),
                                e => allocation.add(data_index).write_unaligned(e),
                            }
                            data_index = data_index + 1;
                        }
                        allocation.add(data_index).write_unaligned(0);
                    }
                }
            }

            string_ptr_slice[len] = std::ptr::null_mut();

            return alloc.cast();
        }
    }
}


///
/// This function copies parts of the string array into a single contiguous memory allocation of the allocator.
///
/// Returns:
/// never return null.
///
/// Info:
/// The resulting pointer is allocated directly by the allocator and must be freed by the caller.
/// Only a single free is sufficient as all strings are placed into a single allocation
/// All pointers in the "strings" member become invalid after the returned pointer is freed.
///
/// Info:
/// All strings are 0 terminated utf-8.
/// Should the strings themselves contain any 0 bytes then those
/// bytes are replaced by a '32' (whitespace) byte.
///
/// Info:
/// NULL elements inside the array are represented as null pointers in the strings member.
///
/// Info:
/// The layout/size of data in opaque_data is an implementation detail.
///
/// Aborts program if:
/// - inst is null
/// - len is 0
/// - the last element to be copied is out of bounds
/// - idx+len overflows
/// - allocator is null
/// - allocator returns null
/// - allocator returns misaligned pointer
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_copy_contiguous(inst: *const StringArray, idx: usize, len: usize, allocator: FFIAllocator) -> *mut StringArrayCopy {
    if len == 0 {
        ffi_abort("StringArray_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("StringArray_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("StringArray_copy was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if needed_length > vec.len() {
                ffi_abort(format!("StringArray_copy index {} out of bounds for array size {}", needed_length-1, vec.len()));
                unreachable!()
            }

            let mut all_strings_size = 0usize;
            for i in 0..len {
                let new_size = all_strings_size.checked_add(vec[idx+i]
                    .as_ref()
                    .map(|a| a.as_bytes().len()+1)
                    .unwrap_or(0));

                if new_size.is_none() {
                    ffi_abort("StringArray_copy overflow when calculating required size");
                    unreachable!()
                }

                all_strings_size = new_size.unwrap();
            }

            let total_size = std::mem::size_of::<StringArrayCopy>() +
                all_strings_size +
                ((len+1) * size_of::<*mut c_char>());

            let alloc = ffi_alloc(allocator,
                                  total_size,
                                  std::mem::align_of::<StringArrayCopy>());

            let struct_ptr : *mut StringArrayCopy = alloc.cast();
            //I cannot use the offset_of macro because cbindgen dies because of it for some reason
            let opaque_data_offset = ((*struct_ptr).opaque_data.as_ptr() as usize) - struct_ptr as usize;

            let struct_ref = struct_ptr.as_mut().unwrap();
            struct_ref.count = len;
            struct_ref.strings = alloc.add(opaque_data_offset).cast();
            let data_ref: *mut u8 = alloc.add(opaque_data_offset + ((len+1) * size_of::<*mut c_char>())).cast();

            let string_ptr_slice = std::slice::from_raw_parts_mut(struct_ref.strings, len+1);
            let data_slice = std::slice::from_raw_parts_mut(data_ref, all_strings_size);

            let mut data_index = 0;
            for i in 0..len {
                match vec[idx+i].as_ref() {
                    None => {
                        string_ptr_slice[i] = std::ptr::null_mut();
                    }
                    Some(data) => {
                        string_ptr_slice[i] = data_ref.add(data_index).cast();
                        for ele in data.as_bytes() {
                            match *ele {
                                0 => data_slice[data_index] = 32,
                                e => data_slice[data_index] = e
                            }
                            data_index = data_index + 1;
                        }
                        data_slice[data_index] = 0;
                        data_index = data_index + 1;
                    }
                }
            }

            string_ptr_slice[len] = std::ptr::null_mut();

            return alloc.cast();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_get_alloc(inst: *const StringArray, idx: usize, allocator: FFIAllocator) -> *mut c_char {
    match inst.as_ref() {
        None => {
            ffi_abort("StringArray_get_alloc was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("StringArray_get_alloc index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            let str = &vec[idx];
            if str.is_none() {
                return std::ptr::null_mut();
            }

            let str = str.as_ref().unwrap();

            let bytes = str.as_bytes();
            let buffer : *mut c_char = ffi_alloc(allocator, bytes.len() + 1, 1).cast();

            for (idx, ele) in bytes.iter().enumerate() {
                match *ele as c_char {
                    0 => buffer.wrapping_add(idx).write_unaligned(32),
                    e => buffer.wrapping_add(idx).write_unaligned(e)
                }
            }
            buffer.wrapping_add(bytes.len()).write_unaligned(0);

            return buffer;
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn StringArray_get(inst: *const StringArray, idx: usize, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("StringArray_get was called with a null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("StringArray_get was called with a null instance pointer");
            unreachable!()
        }

        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("StringArray_get index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            let str = &vec[idx];
            if str.is_none() {
                len.write_unaligned(0);
                return true;
            }

            let str = str.as_ref().unwrap();

            let bytes = str.as_bytes();
            if len.read_unaligned() < bytes.len()+1 {
                len.write_unaligned(bytes.len()+1);
                return false;
            }
            len.write_unaligned(bytes.len()+1);
            if buffer.is_null() {
                return true;
            }
            for (idx, ele) in bytes.iter().enumerate() {
                match *ele as c_char {
                    0 => buffer.wrapping_add(idx).write_unaligned(32),
                    e => buffer.wrapping_add(idx).write_unaligned(e)
                }
            }
            buffer.wrapping_add(bytes.len()).write_unaligned(0);
            return true;
        }
    }
}

impl Deref for StringArray {
    type Target = Vec<OString>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for StringArray {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<&JsonValue> for StringArray {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if !value.is_array() {
            return Err("StringArray expected".to_string());
        }
        let mut array = Vec::with_capacity(value.len());
        for x in value.members() {
            array.push(x.try_into()?);
        }

        return Ok(Self(array))
    }
}

impl Into<JsonValue> for &StringArray {
    fn into(self) -> JsonValue {
        let mut values: Vec<JsonValue> = Vec::with_capacity(self.len());
        for x in &self.0 {
            values.push(x.into());
        }

        return JsonValue::Array(values)
    }
}


impl Into<JsonValue> for StringArray {
    fn into(self) -> JsonValue {
        let mut values: Vec<JsonValue> = Vec::with_capacity(self.len());
        for x in self.0 {
            values.push(x.into());
        }

        return JsonValue::Array(values)
    }
}



#[derive(Debug, Clone, PartialEq)]
pub struct AnyElement(JsonValue);
option_wrapper!(OAnyElement, AnyElement);
as_request_body!(AnyElement);

impl Display for AnyElement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}


impl Eq for AnyElement {

}

impl Default for AnyElement {
    fn default() -> Self {
        AnyElement(JsonValue::Null)
    }
}

impl Hash for AnyElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_string().hash(state)
    }
}

impl TryFrom<JsonValue> for AnyElement {
    type Error = String;
    fn try_from(value: JsonValue) -> Result<Self, String> {
        Ok(AnyElement(value))
    }
}


impl TryFrom<&JsonValue> for AnyElement {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        Ok(AnyElement(value.clone()))
    }
}

impl Into<JsonValue> for &AnyElement {
    fn into(self) -> JsonValue {
        self.0.clone()
    }
}


impl Into<JsonValue> for AnyElement {
    fn into(self) -> JsonValue {
        self.0
    }
}




#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) enum AnyElementType {
    #[default]
    Null,
    Boolean,
    String,
    Integer,
    FloatingPointNumber,
    Object,
    Array,
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
unsafe fn impl_any_element_type(inst: *const AnyElement) -> AnyElementType {
    match inst.as_ref() {
        None => {
            ffi_abort("any_element_type was called with a null pointer");
            unreachable!()
        }

        Some(x) => match &x.0 {
            JsonValue::Null => AnyElementType::Null,
            JsonValue::Short(_) => AnyElementType::Integer,
            JsonValue::String(_) => AnyElementType::String,
            JsonValue::Number(_) => {
                if x.0.as_i64().is_some() && i64::from_str(x.0.to_string().as_str()).is_err() {
                    AnyElementType::Integer
                } else {
                    AnyElementType::FloatingPointNumber
                }
            }
            JsonValue::Boolean(_) => AnyElementType::Boolean,
            JsonValue::Object(_) => AnyElementType::Object,
            JsonValue::Array(_) => AnyElementType::Array,
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_type(inst: *const AnyElement) -> AnyElementType {
    return impl_any_element_type(inst);
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_boolean_get(inst: *const AnyElement) -> bool {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_boolean_get was called with a null pointer");
            unreachable!()
        }

        Some(x) => match &x.0 {
            JsonValue::Boolean(bool) => *bool,
            _=> {
                ffi_abort(format!("AnyElement_boolean_get was called with a pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_integer_get(inst: *const AnyElement) -> i64 {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_integer_get was called with a null pointer");
            unreachable!()
        }

        Some(x) => match &x.0.as_i64() {
            Some(value) => *value,
            _=> {
                ffi_abort(format!("AnyElement_integer_get was called with a pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_double_get(inst: *const AnyElement) -> f64 {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_double_get was called with a null pointer");
            unreachable!()
        }

        Some(x) => match &x.0.as_f64() {
            Some(value) => *value,
            _=> {
                ffi_abort(format!("AnyElement_double_get was called with a pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_string_get(inst: *const AnyElement, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("AnyElement_string_get was called with a null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_string_get was called with a null instance pointer");
            unreachable!()
        }

        Some(x) => match &x.0 {
            JsonValue::String(str) => {
                let bytes = str.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                return true;
            },
            _=> {
                ffi_abort(format!("AnyElement_string_get was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_string_get_alloc(inst: *const AnyElement, allocator: FFIAllocator) -> *mut c_char {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_string_get_alloc was called with a null instance pointer");
            unreachable!()
        }

        Some(x) => match &x.0 {
            JsonValue::String(str) => {
                let bytes = str.as_bytes();
                let buffer : *mut c_char = ffi_alloc(allocator, bytes.len()+1, 1).cast();

                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                return buffer;
            },
            _=> {
                ffi_abort(format!("AnyElement_string_get_alloc was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_size(inst: *const AnyElement) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Array(_) => x.0.len(),
            _=> {
                ffi_abort(format!("AnyElement_array_size was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_get(inst: *const AnyElement, idx: usize) -> *mut AnyElement {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Array(vec) => {
                if idx >= vec.len() {
                    ffi_abort(format!("AnyElement_array_get index {} out of bounds for array size {}", idx, vec.len()));
                    unreachable!()
                }
                Box::into_raw(Box::new(AnyElement((&vec[idx]).clone())))
            },
            _=> {
                ffi_abort(format!("AnyElement_array_get was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_remove(inst: *mut AnyElement, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("AnyElement_array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Array(vec) => {
                if idx >= vec.len() {
                    ffi_abort(format!("AnyElement_array_remove index {} out of bounds for array size {}", idx, vec.len()));
                    unreachable!()
                }

                x.0.array_remove(idx);
            },
            _=> {
                ffi_abort(format!("AnyElement_array_remove was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_add(inst: *mut AnyElement, element: *const AnyElement) {
    if element.is_null() {
        ffi_abort("AnyElement_array_add was called with a element null pointer");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElement_array_add was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &mut x.0 {
            JsonValue::Array(x) => {
                x.push(element.as_ref().unwrap().into());
            },
            _=> {
                ffi_abort(format!("AnyElement_array_add was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_set(inst: *mut AnyElement, idx: usize, element: *const AnyElement) {
    if element.is_null() {
        ffi_abort("AnyElement_array_set was called with a element null pointer");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElement_array_set was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &mut x.0 {
            JsonValue::Array(vec) => {
                if idx >= vec.len() {
                    ffi_abort(format!("AnyElement_array_set index {} out of bounds for array size {}", idx, vec.len()));
                    unreachable!()
                }

                x.0[idx] = element.as_ref().unwrap().into();
            },
            _=> {
                ffi_abort(format!("AnyElement_array_set was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_object_keys(inst: *const AnyElement) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_object_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Object(_) => {
                let size = x.0.len();
                let mut keys = Vec::with_capacity(size);

                for (key, _) in x.0.entries() {
                    keys.push(key.into());
                }

                return Box::into_raw(Box::new(StringArray(keys)));
            }
            _ => {
                ffi_abort(format!("AnyElement_object_keys was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_object_get(inst: *const AnyElement, key: *const c_char) -> *mut AnyElement {
    if key.is_null() {
        ffi_abort("AnyElement_object_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElement_object_get with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("AnyElement_object_get was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Object(o) => {
                let value = o.get(key);
                if value.is_none() {
                    ffi_abort(format!("AnyElement_object_get was called with key {} that does not exists", key));
                    unreachable!()
                }
                Box::into_raw(Box::new(AnyElement(value.unwrap().clone())))
            },
            _ => {
                ffi_abort(format!("AnyElement_object_get was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_object_set(inst: *mut AnyElement, key: *const c_char, value: *const AnyElement) {
    if key.is_null() {
        ffi_abort("AnyElement_object_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElement_object_set with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    if value.is_null() {
        ffi_abort("AnyElement_object_set was called with a value null pointer");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElement_object_set was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &x.0 {
            JsonValue::Object(_) => {
                x.0[key] = value.as_ref().unwrap().into();
            },
            _ => {
                ffi_abort(format!("AnyElement_object_set was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_object_remove(inst: *mut AnyElement, key: *const c_char) {
    if key.is_null() {
        ffi_abort("AnyElement_object_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElement_object_remove with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElement_object_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => match &mut x.0 {
            JsonValue::Object(o) => {
                let value = o.remove(key);
                if value.is_none() {
                    ffi_abort(format!("AnyElement_object_remove was called with key {} that does not exists", key));
                    unreachable!()
                }

            },
            _ => {
                ffi_abort(format!("AnyElement_object_remove was called with a instance pointer to a {:?}", impl_any_element_type(inst)));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_object_new() -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Object(Object::new()))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_array_new() -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Array(Vec::new()))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_string_new(string: *const c_char) -> *mut AnyElement {
    if string.is_null() {
        ffi_abort("AnyElement_new_string was called with a string null pointer");
        unreachable!()
    }

    let string = CStr::from_ptr(string).to_str();
    if string.is_err() {
        ffi_abort("called AnyElement_new_string with string that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let string = string.unwrap();

    Box::into_raw(Box::new(AnyElement(JsonValue::String(string.to_string()))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_null_new() -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Null)))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_double_new(value: f64) -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Number(value.into()))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_integer_new(value: i64) -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Number(value.into()))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_bool_new(value: bool) -> *mut AnyElement {
    Box::into_raw(Box::new(AnyElement(JsonValue::Boolean(value))))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElement_free(value: *mut AnyElement) {
    if value.is_null() {
        ffi_abort("AnyElement_free free(NULL)");
        unreachable!()
    }
    _= Box::from_raw(value);
}


#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct AnyElementMap(pub Map<AnyElement>);
option_wrapper!(OAnyElementMap, AnyElementMap);
as_request_body!(AnyElementMap);

impl Display for AnyElementMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_new() -> *mut AnyElementMap {
    Box::into_raw(Box::new(AnyElementMap::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_free(inst: *mut AnyElementMap) {
    if inst.is_null() {
        ffi_abort("AnyElementMap_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst);
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_remove(inst: *mut AnyElementMap, key: *const c_char) {
    if key.is_null() {
        ffi_abort("AnyElementMap_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElementMap_remove with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElementMap_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(&key).is_none() {
                ffi_abort(format!("AnyElementMap_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_set(inst: *mut AnyElementMap, key: *const c_char, value: *const AnyElement) {
    if key.is_null() {
        ffi_abort("AnyElementMap_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElementMap_set with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElementMap_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = value.as_ref().map(|a| a.clone());
            if value.is_none() {
                ffi_abort("AnyElementMap_set was called with a value null pointer");
                unreachable!()
            }

            map.insert(key, value.unwrap().into());
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_get(inst: *const AnyElementMap, key: *const c_char) -> *mut AnyElement {
    if key.is_null() {
        ffi_abort("AnyElementMap_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("called AnyElementMap_get with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("AnyElementMap_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key).map(|a| a.clone());
            if value.is_none() {
                ffi_abort(format!("AnyElementMap_get was called with key {} that does not exists", key));
                unreachable!()
            }

            Box::into_raw(Box::new(value.unwrap()))
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementMap_keys(inst: *const AnyElementMap) -> *mut StringArray {

    match inst.as_ref() {
        None => {
            ffi_abort("AnyElementMap_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "AnyElementMap_keys")
        }
    }
}

impl Deref for AnyElementMap {
    type Target = LinkedHashMap<String, AnyElement>;
    fn deref(&self) -> &Self::Target {
        &self.0.0
    }
}

impl DerefMut for AnyElementMap {

    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.0
    }
}

impl TryFrom<&JsonValue> for AnyElementMap {

    type Error = String;

    fn try_from(value: &JsonValue) -> Result<Self, String> {
        Ok(Self(value.try_into()?))
    }
}

impl Into<JsonValue> for &AnyElementMap {
    fn into(self) -> JsonValue {
        (&self.0).into()
    }
}


impl Into<JsonValue> for AnyElementMap {
    fn into(self) -> JsonValue {
        self.0.into()
    }
}

#[derive(Debug, Clone, Hash, Default, Eq, PartialEq)]
pub struct AnyElementArray(Vec<AnyElement>);
option_wrapper!(OAnyElementArray, AnyElementArray);
as_request_body!(AnyElementArray);

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_new() -> *mut AnyElementArray {
    Box::into_raw(Box::new(AnyElementArray::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_free(inst: *mut AnyElementArray) {
    if inst.is_null() {
        ffi_abort("AnyElementArray_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_size(inst: *const AnyElementArray) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElementArray_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_get(inst: *const AnyElementArray, idx: usize) -> *mut AnyElement {
    match inst.as_ref() {
        None => {
            ffi_abort("AnyElementArray_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("AnyElementArray_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            Box::into_raw(Box::new(x.0[idx].clone()))
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_set(inst: *mut AnyElementArray, idx: usize, value: *const AnyElement) {
    if value.is_null() {
        ffi_abort("AnyElementArray_set was called with a value null pointer");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElementArray_set was called with a inst null pointer");
            unreachable!()
        }
        Some(vec) => {
            if idx >= vec.len() {
                ffi_abort(format!("AnyElementArray_set index {} out of bounds for array size {}", idx, vec.len()));
                unreachable!()
            }

            vec[idx] = value.as_ref().unwrap().clone();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_remove(inst: *mut AnyElementArray, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("AnyElementArray_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("AnyElementArray_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.0.remove(idx);
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn AnyElementArray_add(inst: *mut AnyElementArray, value: *const AnyElement) {
    if value.is_null() {
        ffi_abort("AnyElementArray_add was called with a value null pointer");
        unreachable!()
    }

    match inst.as_mut() {
        None => {
            ffi_abort("AnyElementArray_add was called with a inst null pointer");
            unreachable!()
        }
        Some(x) => {
            x.0.push(value.as_ref().unwrap().clone())
        }
    }
}

impl Deref for AnyElementArray {
    type Target = Vec<AnyElement>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AnyElementArray {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<&JsonValue> for AnyElementArray {

    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if !value.is_array() {
            return Err("Expected AnyElementArray".to_string());
        }

        let mut array = Vec::with_capacity(value.len());
        for x in value.members() {
            array.push(x.try_into()?);
        }

        Ok(Self(array))
    }
}

impl Into<JsonValue> for &AnyElementArray {
    fn into(self) -> JsonValue {
        return JsonValue::Array(self.0.iter().map(|a| a.into()).collect());
    }
}

impl Into<JsonValue> for AnyElementArray {
    fn into(self) -> JsonValue {
        return JsonValue::Array(self.0.iter().map(|a| a.into()).collect());
    }
}


#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct Map<T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display>(pub LinkedHashMap<String, T>);

impl<T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Deref for Map<T> {
    type Target = LinkedHashMap<String, T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> DerefMut for Map<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Display for Map<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Into<JsonValue> for &Map<T> {
    fn into(self) -> JsonValue {

        let mut new_obj = JsonValue::Object(Object::new());
        for (key, value) in &self.0 {
            let new_value : JsonValue = value.clone().into();
            new_obj[key.as_str()] = new_value;
        }
        return new_obj;
    }
}
impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Into<JsonValue> for Map<T> {
    fn into(self) -> JsonValue {

        let mut new_obj = JsonValue::Object(Object::new());
        for (key, value) in self.0 {
            let new_value : JsonValue = value.into();
            new_obj[key.as_str()] = new_value;
        }
        return new_obj;
    }
}
impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> TryFrom<&JsonValue> for Map<T> {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if !value.is_object() {
            return Err("Expected Map".to_string());
        }

        let mut m: LinkedHashMap<String, T> = LinkedHashMap::new();

        for (key, value) in value.entries() {
            m.insert(key.to_string(), value.try_into()?);
        }

        Ok(Map(m))
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct OMap<T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display>(Option<Map<T>>);

impl <T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Deref for OMap<T> {
    type Target = Option<Map<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl <T: for <'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> DerefMut for OMap<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Display for OMap<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Into<JsonValue> for &OMap<T> {
    fn into(self) -> JsonValue {
        if self.is_none() {
            return JsonValue::Null;
        }

        self.as_ref().unwrap().into()
    }
}
impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> Into<JsonValue> for OMap<T> {
    fn into(self) -> JsonValue {
        if self.is_none() {
            return JsonValue::Null;
        }

        self.0.unwrap().into()
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> From<Map<T>> for OMap<T> {
    fn from(value: Map<T>) -> Self {
        OMap(Some(value))
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> From<&Map<T>> for OMap<T> {
    fn from(value: &Map<T>) -> Self {
        OMap(Some(value.clone()))
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> From<Option<Map<T>>> for OMap<T> {
    fn from(value: Option<Map<T>>) -> Self {
        OMap(value)
    }
}

impl <T: for<'a> TryFrom<&'a JsonValue, Error = String>+Into<JsonValue>+Debug+Clone+Hash+PartialEq+Eq+Default+Display> TryFrom<&JsonValue> for OMap<T> {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if value.is_null() {
            return Ok(OMap(None));
        }

        return Ok(OMap(Some(Map::try_from(value)?)));
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Default)]
pub struct OBool(pub Option<bool>);

impl RequestBody<bool> for OBool {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::Boolean(self.0?).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        if self.0? {
            Some(ApiRequestEntity::String("true".to_string()))
        } else {
            Some(ApiRequestEntity::String("false".to_string()))
        }
    }
}

impl RequestBody<bool> for &OBool {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::Boolean(self.0?).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        if self.0? {
            Some(ApiRequestEntity::String("true".to_string()))
        } else {
            Some(ApiRequestEntity::String("false".to_string()))
        }
    }
}

impl RequestBody<bool> for bool {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::Boolean(self).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        if self {
            Some(ApiRequestEntity::String("true".to_string()))
        } else {
            Some(ApiRequestEntity::String("false".to_string()))
        }
    }
}

impl RequestBody<bool> for &bool {
    fn to_json_body(self) -> Option<ApiRequestEntity> {
        Some(ApiRequestEntity::String(JsonValue::Boolean(*self).to_string()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        if *self {
            Some(ApiRequestEntity::String("true".to_string()))
        } else {
            Some(ApiRequestEntity::String("false".to_string()))
        }
    }
}

impl PathParam<bool> for OBool {
    fn to_path_param_simple(self) -> Option<String> {
        if self.0? {
            Some("true".to_string())
        } else {
            Some("false".to_string())
        }
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        self.to_path_param_simple()
    }

    fn to_path_param_label(self) -> Option<String> {
        if self.0? {
            Some(".true".to_string())
        } else {
            Some(".false".to_string())
        }
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        self.to_path_param_label()
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        if self.0? {
            Some(format!(";{name}=true"))
        } else {
            Some(format!(";{name}=false"))
        }
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        self.to_path_param_matrix(name)
    }
}

impl PathParam<bool> for &OBool {
    fn to_path_param_simple(self) -> Option<String> {
        if self.0? {
            Some("true".to_string())
        } else {
            Some("false".to_string())
        }
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        self.to_path_param_simple()
    }

    fn to_path_param_label(self) -> Option<String> {
        if self.0? {
            Some(".true".to_string())
        } else {
            Some(".false".to_string())
        }
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        self.to_path_param_label()
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        if self.0? {
            Some(format!(";{name}=true"))
        } else {
            Some(format!(";{name}=false"))
        }
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        self.to_path_param_matrix(name)
    }
}

impl PathParam<bool> for bool {
    fn to_path_param_simple(self) -> Option<String> {
        if self {
            Some("true".to_string())
        } else {
            Some("false".to_string())
        }
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        self.to_path_param_simple()
    }

    fn to_path_param_label(self) -> Option<String> {
        if self {
            Some(".true".to_string())
        } else {
            Some(".false".to_string())
        }
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        self.to_path_param_label()
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        if self {
            Some(format!(";{name}=true"))
        } else {
            Some(format!(";{name}=false"))
        }
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        self.to_path_param_matrix(name)
    }
}

impl PathParam<bool> for &bool {
    fn to_path_param_simple(self) -> Option<String> {
        if *self {
            Some("true".to_string())
        } else {
            Some("false".to_string())
        }
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        self.to_path_param_simple()
    }

    fn to_path_param_label(self) -> Option<String> {
        if *self {
            Some(".true".to_string())
        } else {
            Some(".false".to_string())
        }
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        self.to_path_param_label()
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        if *self {
            Some(format!(";{name}=true"))
        } else {
            Some(format!(";{name}=false"))
        }
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        self.to_path_param_matrix(name)
    }
}


impl Deref for OBool {
    type Target = Option<bool>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OBool {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Display for OBool {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}

impl From<bool> for OBool {
    fn from(value: bool) -> Self {
        Self(Some(value))
    }
}

impl From<&bool> for OBool {
    fn from(value: &bool) -> Self {
        Self(Some(*value))
    }
}

impl From<Option<bool>> for OBool {
    fn from(value: Option<bool>) -> Self {
        Self(value)
    }
}

impl TryFrom<&JsonValue> for OBool {
    type Error = String;
    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if let Some(val) = value.as_bool() {
           return Ok(Self(Some(val)));
        }
        if value.is_null() {
            return Ok(Self(None));
        }
        Err("Expected boolean".to_string())
    }
}

impl Into<JsonValue> for OBool {
    fn into(self) -> JsonValue {
        match self.0 {
            None => JsonValue::Null,
            Some(x) => JsonValue::Boolean(x),
        }
    }
}

impl Into<JsonValue> for &OBool {
    fn into(self) -> JsonValue {
        match &self.0 {
            None => JsonValue::Null,
            Some(x) => JsonValue::Boolean(*x),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Default, Ord, PartialOrd)]
pub struct OString(pub Option<String>);

impl RequestBody<String> for OString {

    fn to_json_body(self) -> Option<ApiRequestEntity> {
        if self.0.is_none() {
            return None;
        }
        Some(ApiRequestEntity::String(self.to_json()))
    }

    fn to_text_body(self) -> Option<ApiRequestEntity> {
        if self.0.is_none() {
            return None;
        }
        Some(ApiRequestEntity::String(self.0.unwrap()))
    }
}

impl PathParam<String> for OString {
    fn to_path_param_simple(self) -> Option<String> {
        Some(format!("{}", &self.0?))
    }

    fn to_path_param_simple_explode(self) -> Option<String> {
        self.to_path_param_simple()
    }

    fn to_path_param_label(self) -> Option<String> {
        Some(format!(".{}", &self.0?))
    }

    fn to_path_param_label_explode(self) -> Option<String> {
        Some(format!(".{}", &self.0?))
    }

    fn to_path_param_matrix(self, name: &str) -> Option<String> {
        Some(format!(";{name}={}", &self.0?))
    }

    fn to_path_param_matrix_explode(self, name: &str) -> Option<String> {
        Some(format!(";{name}={}", &self.0?))
    }
}

impl Display for OString {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self.to_json_pretty().as_str(), f)
    }
}

impl Deref for OString {
    type Target = Option<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for OString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TryFrom<OString> for String {
    type Error = ();

    fn try_from(value: OString) -> Result<Self, Self::Error> {
        match value.0 {
            Some(v) => Ok(v),
            None => Err(()),
        }
    }
}

impl From<String> for OString {
    fn from(value: String) -> Self {
        OString(Some(value))
    }
}

impl From<Option<String>> for OString {
    fn from(value: Option<String>) -> Self {
        OString(value)
    }
}

impl From<&str> for OString {
    fn from(value: &str) -> Self {
        Self(Some(value.to_owned()))
    }
}

impl TryFrom<&JsonValue> for OString {

    type Error = String;

    fn try_from(value: &JsonValue) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self(None));
        }

        if let Some(str) = value.as_str() {
            return Ok(Self(Some(str.to_string())));
        }

        Err("String expected".to_string())
    }
}

impl Into<JsonValue> for OString {
    fn into(self) -> JsonValue {
        match self.0 {
            None => JsonValue::Null,
            Some(x) => JsonValue::String(x),
        }
    }
}

impl Into<JsonValue> for &OString {
    fn into(self) -> JsonValue {
        match &self.0 {
            None => JsonValue::Null,
            Some(x) => JsonValue::String(x.to_string()),
        }
    }
}

numeric_type!(OI8, i8, I8Array, OI8Array, OI8Map, I8Map, as_i8);
numeric_type!(OI16, i16, I16Array, OI16Array, OI16Map, I16Map, as_i16);
numeric_type!(OI32, i32, I32Array, OI32Array, OI32Map, I32Map, as_i32);
numeric_type!(OI64, i64, I64Array, OI64Array, OI64Map, I64Map, as_i64);

numeric_type!(OU8, u8, U8Array, OU8Array, OU8Map, U8Map, as_u8);
numeric_type!(OU16, u16, U16Array, OU16Array, OU16Map, U16Map, as_u16);
numeric_type!(OU32, u32, U32Array, OU32Array, OU32Map, U32Map, as_u32);
numeric_type!(OU64, u64, U64Array, OU64Array, OU64Map, U64Map, as_u64);

fp_numeric_type!(OF32, f32, F32Array, OF32Array, OF32Map, F32Map, as_f32);
fp_numeric_type!(OF64, f64, F64Array, OF64Array, OF64Map, F64Map, as_f64);

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_new() -> *mut I8Map {
    Box::into_raw(Box::new(I8Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_free(inst: *mut I8Map) {
    if inst.is_null() {
        ffi_abort("I8Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_get(inst: *const I8Map, key: *const c_char, is_null: *mut bool) -> i8 {
    if key.is_null() {
        ffi_abort("I8Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I8Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I8Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("I8Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_set(inst: *mut I8Map, key: *const c_char, value: *const i8) {
    if key.is_null() {
        ffi_abort("I8Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I8Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("I8Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_remove(inst: *mut I8Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("I8Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I8Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("I8Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("I8Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Map_keys(inst: *const I8Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("I8Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "I8Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_new() -> *mut U8Map {
    Box::into_raw(Box::new(U8Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_free(inst: *mut U8Map) {
    if inst.is_null() {
        ffi_abort("U8Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_get(inst: *const U8Map, key: *const c_char, is_null: *mut bool) -> u8 {
    if key.is_null() {
        ffi_abort("U8Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U8Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U8Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("U8Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_set(inst: *mut U8Map, key: *const c_char, value: *const u8) {
    if key.is_null() {
        ffi_abort("U8Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U8Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("U8Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_remove(inst: *mut U8Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("U8Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U8Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("U8Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("U8Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Map_keys(inst: *const U8Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("U8Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "U8Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_new() -> *mut I16Map {
    Box::into_raw(Box::new(I16Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_free(inst: *mut I16Map) {
    if inst.is_null() {
        ffi_abort("I16Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_get(inst: *const I16Map, key: *const c_char, is_null: *mut bool) -> i16 {
    if key.is_null() {
        ffi_abort("I16Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I16Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I16Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("I16Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_set(inst: *mut I16Map, key: *const c_char, value: *const i16) {
    if key.is_null() {
        ffi_abort("I16Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I16Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("I16Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_remove(inst: *mut I16Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("I16Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I16Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("I16Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("I16Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Map_keys(inst: *const I16Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("I16Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "I16Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_new() -> *mut I16Map {
    Box::into_raw(Box::new(I16Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_free(inst: *mut I16Map) {
    if inst.is_null() {
        ffi_abort("U16Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_get(inst: *const U16Map, key: *const c_char, is_null: *mut bool) -> u16 {
    if key.is_null() {
        ffi_abort("U16Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U16Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U16Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("U16Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_set(inst: *mut U16Map, key: *const c_char, value: *const u16) {
    if key.is_null() {
        ffi_abort("U16Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U16Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("U16Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_remove(inst: *mut U16Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("U16Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U16Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("U16Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("U16Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Map_keys(inst: *const U16Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("U16Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "U16Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_new() -> *mut I32Map {
    Box::into_raw(Box::new(I32Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_free(inst: *mut I32Map) {
    if inst.is_null() {
        ffi_abort("I32Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_get(inst: *const I32Map, key: *const c_char, is_null: *mut bool) -> i32 {
    if key.is_null() {
        ffi_abort("I32Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I32Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I32Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("I32Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_set(inst: *mut I32Map, key: *const c_char, value: *const i32) {
    if key.is_null() {
        ffi_abort("I32Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I32Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("I32Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_remove(inst: *mut I32Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("I32Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I32Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("I32Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("I32Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Map_keys(inst: *const I32Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("I32Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "I32Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_new() -> *mut I32Map {
    Box::into_raw(Box::new(I32Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_free(inst: *mut I32Map) {
    if inst.is_null() {
        ffi_abort("U32Map_new free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_get(inst: *const U32Map, key: *const c_char, is_null: *mut bool) -> u32 {
    if key.is_null() {
        ffi_abort("U32Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U32Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U32Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("U32Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_set(inst: *mut U32Map, key: *const c_char, value: *const u32) {
    if key.is_null() {
        ffi_abort("U32Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U32Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("U32Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_remove(inst: *mut U32Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("U32Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U32Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("U32Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("U32Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Map_keys(inst: *const U32Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("U32Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "U32Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_new() -> *mut I64Map {
    Box::into_raw(Box::new(I64Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_free(inst: *mut I64Map) {
    if inst.is_null() {
        ffi_abort("I64Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_get(inst: *const I64Map, key: *const c_char, is_null: *mut bool) -> i64 {
    if key.is_null() {
        ffi_abort("I64Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I64Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I64Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("I64Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            value.unwrap()
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_set(inst: *mut I64Map, key: *const c_char, value: *const i64) {
    if key.is_null() {
        ffi_abort("I64Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I64Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("I64Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_remove(inst: *mut I64Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("I64Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("I64Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("I64Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("I64Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Map_keys(inst: *const I64Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("I64Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "I64Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_new() -> *mut I64Map {
    Box::into_raw(Box::new(I64Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_free(inst: *mut I64Map) {
    if inst.is_null() {
        ffi_abort("U64Map_new free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_get(inst: *const U64Map, key: *const c_char, is_null: *mut bool) -> u64 {
    if key.is_null() {
        ffi_abort("U64Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U64Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U64Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("U64Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return 0;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_set(inst: *mut U64Map, key: *const c_char, value: *const u64) {
    if key.is_null() {
        ffi_abort("U64Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U64Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("U64Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_remove(inst: *mut U32Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("U32Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("U64Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("U64Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("U64Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Map_keys(inst: *const U64Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("U64Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "U64Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_new() -> *mut F32Map {
    Box::into_raw(Box::new(F32Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_free(inst: *mut F32Map) {
    if inst.is_null() {
        ffi_abort("F32Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_get(inst: *const F32Map, key: *const c_char, is_null: *mut bool) -> f32 {
    if key.is_null() {
        ffi_abort("F32Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F32Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("F32Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("F32Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return f32::NAN;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_set(inst: *mut F32Map, key: *const c_char, value: *const f32) {
    if key.is_null() {
        ffi_abort("F32Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F32Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("F32Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_remove(inst: *mut F32Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("F32Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F32Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("F32Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("F32Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Map_keys(inst: *const F32Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("F32Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "F32Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_new() -> *mut F64Map {
    Box::into_raw(Box::new(F64Map::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_free(inst: *mut F64Map) {
    if inst.is_null() {
        ffi_abort("F64Map_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_get(inst: *const F64Map, key: *const c_char, is_null: *mut bool) -> f64 {
    if key.is_null() {
        ffi_abort("F64Map_get was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F64Map_get called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("F64Map_get was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            let value = map.get(key);
            if value.is_none() {
                ffi_abort(format!("F64Map_get was called with key {} that does not exists", key));
                unreachable!()
            }

            let value = value.unwrap();
            if value.is_none() {
                if !is_null.is_null() {
                    *is_null = true;
                }

                return f64::NAN;
            }
            if !is_null.is_null() {
                *is_null = false;
            }

            return value.unwrap();
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_set(inst: *mut F64Map, key: *const c_char, value: *const f64) {
    if key.is_null() {
        ffi_abort("F64Map_set was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F64Map_set called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap().to_string();

    match inst.as_mut() {
        None => {
            ffi_abort("F64Map_set was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if value.is_null() {
                map.insert(key, None.into());
            } else {
                map.insert(key, value.read_unaligned().into());
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_remove(inst: *mut F32Map, key: *const c_char) {
    if key.is_null() {
        ffi_abort("F64Map_remove was called with a key null pointer");
        unreachable!()
    }

    let key = CStr::from_ptr(key).to_str();
    if key.is_err() {
        ffi_abort("F64Map_remove called with key that is not valid 0 terminated utf-8. Pointer my be invalid?");
        unreachable!()
    }

    let key = key.unwrap();

    match inst.as_mut() {
        None => {
            ffi_abort("F64Map_remove was called with a inst null pointer");
            unreachable!()
        }
        Some(map) => {
            if map.remove(key).is_none() {
                ffi_abort(format!("F64Map_remove was called with key {} that does not exists", key));
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Map_keys(inst: *const F64Map) -> *mut StringArray {
    match inst.as_ref() {
        None => {
            ffi_abort("F64Map_keys was called with a inst null pointer");
            unreachable!()
        }
        Some(any_map) => {
            ffi_get_map_keys(&any_map.0, "F64Map_keys")
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_new() -> *mut I8Array {
    Box::into_raw(Box::new(I8Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_free(inst: *mut I8Array) {
    if inst.is_null() {
        ffi_abort("I8Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_size(inst: *const I8Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("I8Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_add(inst: *mut I8Array, value: *const i8) {
    match inst.as_mut() {
        None => {
            ffi_abort("I8Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OI8::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_remove(inst: *mut I8Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("I8Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I8Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_set(inst: *mut I8Array, idx: usize, value: *const i8) {
    match inst.as_mut() {
        None => {
            ffi_abort("I8Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I8Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OI8::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_copy(inst: *const I8Array, idx: usize, buffer: *mut i8, len: usize) {
    if len == 0 {
        ffi_abort("I8Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("I8Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I8Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("I8Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(i8::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I8Array_get(inst: *const I8Array, idx: usize, is_null: *mut bool) -> i8 {
    match inst.as_ref() {
        None => {
            ffi_abort("I8Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I8Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    i8::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_new() -> *mut U8Array {
    Box::into_raw(Box::new(U8Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_free(inst: *mut U8Array) {
    if inst.is_null() {
        ffi_abort("U8Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_size(inst: *const U8Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("U8Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_add(inst: *mut U8Array, value: *const u8) {
    match inst.as_mut() {
        None => {
            ffi_abort("U8Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OU8::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_remove(inst: *mut U8Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("U8Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U8Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_set(inst: *mut U8Array, idx: usize, value: *const u8) {
    match inst.as_mut() {
        None => {
            ffi_abort("U8Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U8Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OU8::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_copy(inst: *const U8Array, idx: usize, buffer: *mut u8, len: usize) {
    if len == 0 {
        ffi_abort("U8Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("U8Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U8Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("U8Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(u8::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U8Array_get(inst: *const U8Array, idx: usize, is_null: *mut bool) -> u8 {
    match inst.as_ref() {
        None => {
            ffi_abort("U8Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U8Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    u8::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_new() -> *mut I16Array {
    Box::into_raw(Box::new(I16Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_free(inst: *mut I16Array) {
    if inst.is_null() {
        ffi_abort("I16Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_size(inst: *const I16Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("I16Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_add(inst: *mut I16Array, value: *const i16) {
    match inst.as_mut() {
        None => {
            ffi_abort("I16Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OI16::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_remove(inst: *mut I16Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("I16Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I16Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_set(inst: *mut I16Array, idx: usize, value: *const i16) {
    match inst.as_mut() {
        None => {
            ffi_abort("I16Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I16Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OI16::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_copy(inst: *const I16Array, idx: usize, buffer: *mut i16, len: usize) {
    if len == 0 {
        ffi_abort("I16Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("I16Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I16Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("I16Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(i16::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I16Array_get(inst: *const I16Array, idx: usize, is_null: *mut bool) -> i16 {
    match inst.as_ref() {
        None => {
            ffi_abort("I16Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I16Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    i16::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_new() -> *mut U16Array {
    Box::into_raw(Box::new(U16Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_free(inst: *mut U16Array) {
    if inst.is_null() {
        ffi_abort("U16Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_size(inst: *const U16Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("U16Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_add(inst: *mut U16Array, value: *const u16) {
    match inst.as_mut() {
        None => {
            ffi_abort("U16Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OU16::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_remove(inst: *mut U16Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("U16Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U16Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_set(inst: *mut U16Array, idx: usize, value: *const u16) {
    match inst.as_mut() {
        None => {
            ffi_abort("U16Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U16Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OU16::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_copy(inst: *const U16Array, idx: usize, buffer: *mut u16, len: usize) {
    if len == 0 {
        ffi_abort("U16Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("U16Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U16Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("U16Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(u16::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U16Array_get(inst: *const U16Array, idx: usize, is_null: *mut bool) -> u16 {
    match inst.as_ref() {
        None => {
            ffi_abort("U16Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U16Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    u16::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_new() -> *mut I32Array {
    Box::into_raw(Box::new(I32Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_free(inst: *mut I32Array) {
    if inst.is_null() {
        ffi_abort("I32Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_size(inst: *const I32Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("I32Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_add(inst: *mut I32Array, value: *const i32) {
    match inst.as_mut() {
        None => {
            ffi_abort("I32Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OI32::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_remove(inst: *mut I32Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("I32Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I32Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_set(inst: *mut I32Array, idx: usize, value: *const i32) {
    match inst.as_mut() {
        None => {
            ffi_abort("I32Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I32Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OI32::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_copy(inst: *const I32Array, idx: usize, buffer: *mut i32, len: usize) {
    if len == 0 {
        ffi_abort("I32Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("I32Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I32Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("I32Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(i32::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I32Array_get(inst: *const I32Array, idx: usize, is_null: *mut bool) -> i32 {
    match inst.as_ref() {
        None => {
            ffi_abort("I32Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I32Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    i32::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_new() -> *mut U32Array {
    Box::into_raw(Box::new(U32Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_free(inst: *mut U32Array) {
    if inst.is_null() {
        ffi_abort("U32Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_size(inst: *const U32Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("U32Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_add(inst: *mut U32Array, value: *const u32) {
    match inst.as_mut() {
        None => {
            ffi_abort("U32Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OU32::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_remove(inst: *mut U32Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("U32Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U32Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_set(inst: *mut U32Array, idx: usize, value: *const u32) {
    match inst.as_mut() {
        None => {
            ffi_abort("U32Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U32Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OU32::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_copy(inst: *const U32Array, idx: usize, buffer: *mut u32, len: usize) {
    if len == 0 {
        ffi_abort("U32Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("U32Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U32Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("U32Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(u32::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U32Array_get(inst: *const U32Array, idx: usize, is_null: *mut bool) -> u32 {
    match inst.as_ref() {
        None => {
            ffi_abort("U32Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U32Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    u32::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_new() -> *mut I64Array {
    Box::into_raw(Box::new(I64Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_free(inst: *mut I64Array) {
    if inst.is_null() {
        ffi_abort("I64Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_size(inst: *const I64Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("I64Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_add(inst: *mut I64Array, value: *const i64) {
    match inst.as_mut() {
        None => {
            ffi_abort("I64Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OI64::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_remove(inst: *mut I64Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("I64Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I64Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_set(inst: *mut I64Array, idx: usize, value: *const i64) {
    match inst.as_mut() {
        None => {
            ffi_abort("I64Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I64Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OI64::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_copy(inst: *const I64Array, idx: usize, buffer: *mut i64, len: usize) {
    if len == 0 {
        ffi_abort("I64Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("I64Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("I64Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("I64Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(i64::default()));
            }
        }
    }
}


#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn I64Array_get(inst: *const I64Array, idx: usize, is_null: *mut bool) -> i64 {
    match inst.as_ref() {
        None => {
            ffi_abort("I64Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("I64Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    i64::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_new() -> *mut U64Array {
    Box::into_raw(Box::new(U64Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_free(inst: *mut U64Array) {
    if inst.is_null() {
        ffi_abort("U64Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_size(inst: *const U64Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("U64Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_add(inst: *mut U64Array, value: *const u64) {
    match inst.as_mut() {
        None => {
            ffi_abort("U64Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OU64::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_remove(inst: *mut U64Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("U64Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U64Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_set(inst: *mut U64Array, idx: usize, value: *const u64) {
    match inst.as_mut() {
        None => {
            ffi_abort("U64Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U64Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OU64::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

///
/// Copy a range from the array.
/// Any NULL elements inside the array will be treated as if it was the default value (0).
///
///
/// Aborts program if:
/// - inst is null
/// - buffer is null
/// - len is 0
/// - idx+len overflows
/// - idx+len > array length (out of bounds)
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_copy(inst: *const U64Array, idx: usize, buffer: *mut u64, len: usize) {
    if len == 0 {
        ffi_abort("U64Array_copy len == 0");
        unreachable!()
    }

    let needed_length = idx.checked_add(len);
    if needed_length.is_none() {
        ffi_abort("U64Array_copy idx+len overflows");
        unreachable!()
    }

    let needed_length = needed_length.unwrap();

    match inst.as_ref() {
        None => {
            ffi_abort("U64Array_copy was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if needed_length > x.len() {
                ffi_abort(format!("U64Array_copy index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            for target_idx in 0..len {
                buffer.wrapping_add(target_idx)
                    .write_unaligned(x[idx + target_idx].unwrap_or(u64::default()));
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn U64Array_get(inst: *const U64Array, idx: usize, is_null: *mut bool) -> u64 {
    match inst.as_ref() {
        None => {
            ffi_abort("U64Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U64Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    u64::default()
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_new() -> *mut F64Array {
    Box::into_raw(Box::new(F64Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_free(inst: *mut F64Array) {
    if inst.is_null() {
        ffi_abort("F64Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_size(inst: *const F64Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("F64Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_add(inst: *mut F64Array, value: *const f64) {
    match inst.as_mut() {
        None => {
            ffi_abort("F64Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OF64::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_remove(inst: *mut F64Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("F64Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("F64Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_set(inst: *mut F64Array, idx: usize, value: *const f64) {
    match inst.as_mut() {
        None => {
            ffi_abort("U64Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("U64Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OF64::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F64Array_get(inst: *const F64Array, idx: usize, is_null: *mut bool) -> f64 {
    match inst.as_ref() {
        None => {
            ffi_abort("F64Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("F64Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    f64::NAN
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_new() -> *mut F32Array {
    Box::into_raw(Box::new(F32Array::default()))
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_free(inst: *mut F32Array) {
    if inst.is_null() {
        ffi_abort("F32Array_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_size(inst: *const F32Array) -> usize {
    match inst.as_ref() {
        None => {
            ffi_abort("F32Array_size was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.len()
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_add(inst: *mut F32Array, value: *const f32) {
    match inst.as_mut() {
        None => {
            ffi_abort("F32Array_add was called with a null pointer");
            unreachable!()
        }
        Some(x) => x.push(match value.as_ref() {
            None => OF32::default(),
            Some(val) => (*val).into(),
        })
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_remove(inst: *mut F32Array, idx: usize) {
    match inst.as_mut() {
        None => {
            ffi_abort("F32Array_remove was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("F32Array_remove index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x.remove(idx);
        },
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_set(inst: *mut F32Array, idx: usize, value: *const f32) {
    match inst.as_mut() {
        None => {
            ffi_abort("F32Array_set was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("F32Array_set index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            x[idx] = match value.as_ref() {
                None => OF32::default(),
                Some(val) => (*val).into(),
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn F32Array_get(inst: *const F32Array, idx: usize, is_null: *mut bool) -> f32 {
    match inst.as_ref() {
        None => {
            ffi_abort("F32Array_get was called with a null pointer");
            unreachable!()
        }
        Some(x) => {
            if idx >= x.len() {
                ffi_abort(format!("F32Array_get index {} out of bounds for array size {}", idx, x.len()));
                unreachable!()
            }

            match x[idx].0 {
                None => {
                    if !is_null.is_null() {
                        *is_null = true;
                    }

                    f32::NAN
                }
                Some(value) => {
                    if !is_null.is_null() {
                        *is_null = false;
                    }

                    value
                }
            }
        }
    }
}

pub trait FromJsonString : Sized {
    fn from_json(str: &str) -> Result<Self, json::Error>;
}

pub trait ToJsonString : Sized {
    fn to_json(&self) -> String;

    fn to_json_pretty(&self) -> String;
}

impl<T: for<'a> TryFrom<&'a JsonValue, Error = String>+Sized> FromJsonString for T {
    fn from_json(str: &str) -> Result<Self, json::Error> {
        Ok(T::try_from(&(json::parse(str)?)).map_err(|e| json::Error::wrong_type(e.as_str()))?)
    }
}

impl<T> ToJsonString for T where for<'a> &'a T: Into<JsonValue>{
    fn to_json(&self) -> String {
        json::stringify(self)
    }

    fn to_json_pretty(&self) -> String {
        json::stringify_pretty(self, 4)
    }
}

pub trait AnyRef: Send+Sync+Debug {
    fn as_any_ref(&self) -> Box<&dyn Any>;
    fn as_any_ref_mut(&mut self) -> Box<&mut dyn Any>;
}

pub trait RequestCustomizer: AnyRef {
    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_request_blocking(&self, operation_id: &str, client: &reqwest::blocking::Client, request: ApiRequestBuilder) -> Result<ApiRequestBuilder, ApiError>;
    
    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    fn customize_request_async(&self, _operation_id: &str, _client: &reqwest::Client, request: ApiRequestBuilder) -> Result<ApiRequestBuilder, ApiError>;

    fn clone_to_box(&self) -> Box<dyn RequestCustomizer>;
}

pub trait ResponseCustomizer: AnyRef {

    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_response_blocking(&self, operation_id: &str,
                                   client: &reqwest::blocking::Client,
                                   request_url: &reqwest::Url,
                                   request_headers: &HeaderMap,
                                   response: reqwest::blocking::Response)
        -> Result<either::Either<reqwest::blocking::Response, Box<dyn Any+Send>>, ApiError>;

    #[cfg(feature = "async")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_response_async(&self,
                                operation_id: &str,
                                client: &reqwest::Client,
                                request_url: &reqwest::Url,
                                request_headers: &HeaderMap,
                                response: reqwest::Response)
        -> std::pin::Pin<Box<dyn Future<Output = either::Either<reqwest::Response,Result<Box<dyn Any+Send>, ApiError>>> + Send>>;


    #[cfg(target_arch = "wasm32")]
    fn customize_response_async(&self,
                                operation_id: &str,
                                client: &reqwest::Client,
                                request_url: &reqwest::Url,
                                request_headers: &HeaderMap,
                                response: reqwest::Response)
        -> std::pin::Pin<Box<dyn Future<Output = either::Either<reqwest::Response,Result<Box<dyn Any+Send>, ApiError>>>>>;

    fn clone_to_box(&self) -> Box<dyn ResponseCustomizer>;
}

pub enum ApiRequestEntity {
    Buffer(Vec<u8>),
    String(String),
    Stream(Stream)
}

impl Debug for ApiRequestEntity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiRequestEntity::Buffer(obj) => Debug::fmt(obj, f),
            ApiRequestEntity::String(obj) => Debug::fmt(obj, f),
            ApiRequestEntity::Stream(_) => f.write_str("Box<dyn Read>"),
        }
    }
}


#[derive(Debug)]
pub enum ApiError {
    InvalidRequestHeader(String, String, ApiRequestBuilder),
    InvalidRequestMethod(String, ApiRequestBuilder),
    ReqwestError(reqwest::Error),
    JsonError(json::Error, reqwest::Url, HeaderMap, StatusCode, HeaderMap, String),
    UnexepectedJsonData(JsonValue, HeaderMap, StatusCode, HeaderMap, String),
    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    UnexpectedStatusCodeBlocking(reqwest::Url, HeaderMap, reqwest::blocking::Response),
    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    UnexpectedContentTypeBlocking(reqwest::Url, HeaderMap, reqwest::blocking::Response),
    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    UnexpectedStatusCodeAsync(reqwest::Url, HeaderMap, reqwest::Response),
    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    UnexpectedContentTypeAsync(reqwest::Url, HeaderMap, reqwest::Response),
    Other(Box<dyn Any+Sync+Send>),
}

#[repr(C)]
#[derive(Debug)]
pub enum ApiErrorType {
    ApiErrorInvalidRequestHeader,
    ApiErrorInvalidRequestMethod,
    ApiErrorReqwestError,
    ApiErrorJsonError,
    ApiErrorUnexpectedJsonData,
    ApiErrorUnexpectedStatusCode,
    ApiErrorUnexpectedContentType,
    ApiErrorOther,
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_free(inst: *mut ApiError) {
    if inst.is_null() {
        ffi_abort("api_error_free free(NULL)");
        unreachable!()
    }
    _=Box::from_raw(inst)
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_type(inst: *const ApiError) -> ApiErrorType {
    match inst.as_ref() {
        None => {
            ffi_abort("api_error_type called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::InvalidRequestHeader(_, _, _) => ApiErrorType::ApiErrorInvalidRequestHeader,
            ApiError::InvalidRequestMethod(_, _) => ApiErrorType::ApiErrorInvalidRequestMethod,
            ApiError::ReqwestError(_) => ApiErrorType::ApiErrorReqwestError,
            ApiError::JsonError(_, _, _, _, _, _) => ApiErrorType::ApiErrorJsonError,
            ApiError::UnexepectedJsonData(_, _, _, _, _) => ApiErrorType::ApiErrorUnexpectedJsonData,
            ApiError::UnexpectedStatusCodeBlocking(_, _, _) => ApiErrorType::ApiErrorUnexpectedStatusCode,
            ApiError::UnexpectedContentTypeBlocking(_, _, _) => ApiErrorType::ApiErrorUnexpectedContentType,
            _ => ApiErrorType::ApiErrorOther,
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_invalid_request_header_key(inst: *const ApiError, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("api_error_invalid_request_header_key called with null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("api_error_invalid_request_header_key called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::InvalidRequestHeader(key, _, _) => {

                let bytes = key.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                true
            },
            _=> {
                ffi_abort("api_error_invalid_request_header_key called with error that is not ApiError::InvalidRequestHeader");
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_invalid_request_header_value(inst: *const ApiError, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("api_error_invalid_request_header_value called with null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("api_error_invalid_request_header_value called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::InvalidRequestHeader(_, value, _) => {

                let bytes = value.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                true
            },
            _=> {
                ffi_abort("api_error_invalid_request_header_value called with error that is not ApiError::InvalidRequestHeader");
                unreachable!()
            }
        }
    }
}

#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_invalid_request_method_name(inst: *const ApiError, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("api_error_invalid_request_method_name called with null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("api_error_invalid_request_method_name called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::InvalidRequestMethod(name, _) => {

                let bytes = name.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                true
            },
            _=> {
                ffi_abort("api_error_invalid_request_method_name called with error that is not ApiError::InvalidRequestMethod");
                unreachable!()
            }
        }
    }
}

///
/// Gets a string representation of the error that occurred inside the request framework.
/// param buffer NULL or buffer to memory to store the string
/// param len. pointer to an uintptr_t that contains the length of the buffer.
///
/// NOTE: should any 0 byte be uncounted within the error message then it will be substituted with a 32 byte
/// which represents an ASCII whitespace char.
///
/// NOTE: the returned String entirely depends on whatever the request framework decides to output.
/// This may change and should not be relied upon for logging.
///
/// NOTE: No guarantees are made that subsequent calls to this method will yield a string
/// with the same length as the previous call. It can be assumed to be the case
/// but this depends on the internals of the reqwest framework as well as the type of
/// internal error that has occurred. Defensive implementations should call
/// this method in a while loop and give up after a reasonable number of unsuccessful attempts
/// have been made to retrieve the error. (This function calls Debug::fmt on request::Error)
///
/// Returns:
/// true if the method was successful.
/// false if not.
///
/// In both cases the len pointer will have the required/needed amount of bytes written to it.
/// This amount includes the terminating 0 bytes.
///
/// Aborts program if:
/// inst is null
/// len is null
/// inst does not refer to a ApiError::ReqwestError
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_reqwest_error(inst: *const ApiError, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("api_error_invalid_request_reqwest_error called with null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("api_error_invalid_request_reqwest_error called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::ReqwestError(error) => {
                let fby = format!("{:?}", error);
                let bytes = fby.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                true
            },
            _=> {
                ffi_abort("api_error_invalid_request_reqwest_error called with error that is not ApiError::ReqwestError");
                unreachable!()
            }
        }
    }
}

///
/// Fetch the raw json body as a zero terminated UTF-8 String.
///
/// param buffer NULL or buffer to memory to store the string
/// param len. pointer to an uintptr_t that contains the length of the buffer.
///
/// NOTE: should any 0 byte be uncounted within the JSON body then it will be substituted with a 32 byte
/// which represents an ASCII whitespace char. If you happen to determine the returned json to be valid
/// json then this could be the cause as the JSON parser will stop if it encounters a 0 byte in the response.
///
///
///
/// Returns:
/// true if the method was successful.
/// false if not.
///
/// In both cases the len pointer will have the required/needed amount of bytes written to it.
/// This amount includes the terminating 0 bytes.
///
/// Aborts program if:
/// inst is null
/// len is null
/// inst does not refer to a ApiError::JsonError
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_json_error_get_raw_json(inst: *const ApiError, buffer: *mut c_char, len: *mut usize) -> bool {
    if len.is_null() {
        ffi_abort("api_error_json_error_get_raw_json called with null len pointer");
        unreachable!()
    }

    match inst.as_ref() {
        None => {
            ffi_abort("api_error_json_error_get_raw_json called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::JsonError(_, _, _, _, _, raw_json) => {
                let bytes = raw_json.as_bytes();
                if len.read_unaligned() < bytes.len()+1 {
                    len.write_unaligned(bytes.len()+1);
                    return false;
                }
                len.write_unaligned(bytes.len()+1);
                if buffer.is_null() {
                    return true;
                }
                for (idx, ele) in bytes.iter().enumerate() {
                    match *ele as c_char {
                        0 => buffer.wrapping_add(idx).write_unaligned(32),
                        e => buffer.wrapping_add(idx).write_unaligned(e)
                    }
                }
                buffer.wrapping_add(bytes.len()).write_unaligned(0);
                true
            },
            _=> {
                ffi_abort("api_error_json_error_get_raw_json called with error that is not ApiError::JsonError");
                unreachable!()
            }
        }
    }
}

///
/// Get all HTTP response headers that were present in the HTTP response
/// Return Value:
/// StringMap containing the Key->Value pairs of each http header.

/// This function never returns NULL.
///
/// The returned map will only contain the first header value for each key.
/// If the first Header Value contains a non US-ASCII byte then
/// it is returned as a NULL/Absent value in the StringMap.
///
/// Note: Header Keys will never contain non US-ASCII bytes as the header section will already
/// be validated to that extend if you get any of the valid inputs to this function.
///
/// Aborts program if:
/// inst is null
/// inst does not refer to ApiError::JsonError, ApiError::UnexpectedStatusCode or ApiError::UnexpectedContentType
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_response_headers(inst: *const ApiError) -> *mut StringMap {
    match inst.as_ref() {
        None => {
            ffi_abort("api_error_response_headers called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::JsonError(_, _, _, _, headers, _) => {
                let mut mapped = StringMap::default();
                for (k, v) in headers.iter() {
                    mapped.insert(k.to_string(), v.to_str().ok().map(|e| e.into()).unwrap_or(OString::default()));
                }

                Box::into_raw(Box::new(mapped))
            },
            ApiError::UnexpectedStatusCodeBlocking(_, _, res) | ApiError::UnexpectedContentTypeBlocking(_, _, res) => {
                let mut mapped = StringMap::default();
                for (k, v) in res.headers().iter() {
                    mapped.insert(k.to_string(), v.to_str().ok().map(|e| e.into()).unwrap_or(OString::default()));
                }

                Box::into_raw(Box::new(mapped))
            }
            _=> {
                ffi_abort("api_error_response_headers called with error that is not ApiError::JsonError, ApiError::UnexpectedStatusCode or ApiError::UnexpectedContentType");
                unreachable!()
            }
        }
    }
}

///
/// Get the HTTP status code as an u16.
/// Returns:
/// The HTTP status code as an u16.
/// The returned value is guaranteed to be between (inclusive) 0 and 999.
/// Under most normal circumstances this value is between (inclusive) 100 and 599.
///
/// aborts program if:
/// inst is null
/// inst does not refer to ApiError::JsonError, ApiError::UnexpectedStatusCode or ApiError::UnexpectedContentType
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_status_code(inst: *const ApiError) -> u16 {
    let status = match inst.as_ref() {
        None => {
            ffi_abort("api_error_status_code called with null inst pointer");
            unreachable!()
        }
        Some(inst) => match inst {
            ApiError::JsonError(_, _, _, status, _, _) => {
                status.as_u16()
            },
            ApiError::UnexpectedStatusCodeBlocking(_, _, res) => {
                res.status().as_u16()
            }
            ApiError::UnexpectedContentTypeBlocking(_, _, res) => {
                res.status().as_u16()
            }
            _=> {
                ffi_abort("api_error_status_code called with error that is not ApiError::JsonError, ApiError::UnexpectedStatusCodeBlocking or ApiError::UnexpectedContentTypeBlocking");
                unreachable!()
            }
        }
    };

    if status < 999 {
        status
    } else {
        999
    }
}

///
/// This function will return a Stream to the data in the response and free the error.
///
/// IMPORTANT:
/// !!!!! DO NOT CALL ApiError_free() AFTER CALLING THIS FUNCTION !!!!
/// This function will free the ApiError so accessing headers or other information from the ApiError will NOT be possible anymore.
/// This is the only function for handling ApiError's that does have this characteristic!
///
/// NOTE: The data has not been transferred yet over the network!
/// This function should not be called after a significant delay
/// if one wishes to still obtain the data from the server!
/// Exact Timeout Semantics depend on the server!
/// The returned Stream may fail to read data if such a network timeout occured.
///
/// Returns:
/// A String to read all the response body data from.
/// This may return an empty stream if the response body is empty.
/// This function never returns null.
///
/// aborts program if:
/// inst is null
/// inst does not refer to ApiError::UnexpectedStatusCode or ApiError::UnexpectedContentType
///
#[cfg(all(feature = "ffi", feature = "blocking"))]
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle] pub(crate) unsafe extern "C" fn ApiError_stream_body_and_free(inst: *mut ApiError) -> *mut Stream {
    if inst.is_null() {
        ffi_abort("ApiError_stream_body_and_free called with null inst pointer");
        unreachable!()
    }
    let error = *Box::from_raw(inst);

    match error {
        ApiError::UnexpectedStatusCodeBlocking(_, _, res) | ApiError::UnexpectedContentTypeBlocking(_, _, res)=> {
            Box::into_raw(Box::new(Stream::from(Box::new(res) as Box<dyn std::io::Read+Send>)))
        }
        _=> {
            ffi_abort("ApiError_stream_body_and_free called with error that is not ApiError::UnexpectedStatusCode or ApiError::UnexpectedContentType");
            unreachable!()
        }
    }
}


impl From<reqwest::Error> for ApiError {
    fn from(value: reqwest::Error) -> Self {
        ApiError::ReqwestError(value)
    }
}

impl ApiError {
    pub fn other(message: &str) -> ApiError {
        ApiError::Other(Box::new(message.to_string()))
    }
}

fn get_content_type(response_headers: &HeaderMap) -> Option<&[u8]> {
    let raw_header = response_headers.get("content-type").map(|h| h.as_bytes());
    if raw_header.is_none() {
        return None;
    }

    let raw_header = raw_header.unwrap();

    for i in 0 .. raw_header.len() {
        if raw_header[i] == 0x3B {
            return Some(&raw_header[0..i]);
        }
    }

    Some(raw_header)
}



#[derive(Default, Debug)]
pub struct ApiRequestBuilder {
    pub path: String,
    pub method: String,
    pub headers: HeaderMap,
    pub query: Vec<(String, String)>,
    pub path_parameters: HashMap<String, String>,
    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    builder_blocking: Option<DebugWrapper<Box<dyn FnOnce(&reqwest::blocking::Client, Method, String) -> reqwest::blocking::RequestBuilder>>>,
    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    builder_async: Option<DebugWrapper<Box<dyn FnOnce(&reqwest::Client, Method, String) -> reqwest::RequestBuilder>>>,
    pub entity: Option<ApiRequestEntity>
}

#[repr(transparent)]
struct DebugWrapper<T>(T);

impl<T> Deref for DebugWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for DebugWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Debug for DebugWrapper<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("DebugWrapper")
    }
}

impl ApiRequestBuilder {

    pub fn method<T: ToString>(mut self, method: T) -> ApiRequestBuilder {
        self.method = method.to_string();
        self
    }
    pub fn path<T: ToString>(mut self, path: T) -> ApiRequestBuilder {
        self.path = path.to_string();
        self
    }

    pub fn set_header<K: ToString, V: ToString>(mut self, key: K, value: V) -> Result<ApiRequestBuilder, ApiError> {
        let key = key.to_string();
        let value= value.to_string();
        let header = HeaderValue::from_str(value.as_str());
        if header.is_err() {
            return Err(ApiError::InvalidRequestHeader(key, value, self));
        }

        let header_name = HeaderName::from_str(key.as_str());
        if header_name.is_err() {
            return Err(ApiError::InvalidRequestHeader(key, value, self));
        }

        self.headers.insert(header_name.unwrap(), header.unwrap());
        Ok(self)
    }

    pub fn add_header<K: ToString, V: ToString>(mut self, key: K, value: V) -> Result<ApiRequestBuilder, ApiError> {
        let key = key.to_string();
        let value= value.to_string();
        let header = HeaderValue::from_str(value.as_str());
        if header.is_err() {
            return Err(ApiError::InvalidRequestHeader(key, value, self));
        }

        let header_name = HeaderName::from_str(key.as_str());
        if header_name.is_err() {
            return Err(ApiError::InvalidRequestHeader(key, value, self));
        }

        let header_name = header_name.unwrap();

        if self.headers.contains_key(&header_name) {
            self.headers.append(header_name, header.unwrap());
        } else {
            self.headers.insert(header_name, header.unwrap());
        }

        Ok(self)
    }

    pub fn add_optional_header<K: ToString, V: ToString>(self, key: K, value: Option<V>) -> Result<ApiRequestBuilder, ApiError> {
        if value.is_none() {
            return Ok(self)
        }

        self.add_header(key, value.unwrap())
    }

    pub fn entity_str<T: ToString>(self, entity: T) -> ApiRequestBuilder {
        self.entity_string(entity.to_string())
    }

    pub fn entity_string(mut self, entity: String) -> ApiRequestBuilder {
        self.entity = Some(ApiRequestEntity::String(entity));
        self
    }

    pub fn entity_json<X: Debug, T: RequestBody<X>>(mut self, entity: T) -> ApiRequestBuilder {
        self.entity = entity.to_json_body();
        self
    }

    pub fn entity_stream(mut self, entity: OStream) -> ApiRequestBuilder {
        if entity.0.is_none() {
            return self;
        }
        self.entity = Some(ApiRequestEntity::Stream(entity.0.unwrap()));
        self
    }

    pub fn entity_buffer(mut self, entity: Vec<u8>) -> ApiRequestBuilder {
        self.entity = Some(ApiRequestEntity::Buffer(entity));
        self
    }

    pub fn add_path_param<A: ToString, B: ToString>(mut self, key: A, value: B) -> ApiRequestBuilder {
        self.path_parameters.insert(key.to_string(), value.to_string());
        self
    }

    pub fn add_query<A: ToString, B: ToString>(mut self, key: A, value: B) -> ApiRequestBuilder {
        self.query.push((key.to_string(), value.to_string()));
        self
    }

    pub fn add_optional_query<A: ToString, B: ToString>(mut self, key: A, value: Option<B>) -> ApiRequestBuilder {
        if value.is_some() {
            self.query.push((key.to_string(), value.unwrap().to_string()));
        }
        self
    }

    pub fn add_string_array_query<A: ToString>(mut self, key: A, value: &OStringArray) -> ApiRequestBuilder {
        let key = key.to_string();
        if let Some(value) = value.0.as_ref() {
            for n in value.0.iter() {
                if let Some(value) = n.0.as_ref() {
                    self.query.push((key.clone(), value.clone()));
                }
            };
        };

        self
    }

    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_builder_factory_blocking(mut self, builder_factory: impl FnOnce(&reqwest::blocking::Client, Method, String) -> reqwest::blocking::RequestBuilder + 'static) -> Self {
        self.builder_blocking = Some(DebugWrapper(Box::new(builder_factory)));
        self
    }

    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    pub fn set_builder_factory_async(mut self, builder_factory: impl FnOnce(&reqwest::Client, Method, String) -> reqwest::RequestBuilder + 'static) -> Self {
        self.builder_async = Some(DebugWrapper(Box::new(builder_factory)));
        self
    }

    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    pub async fn build_async<T: ToString>(self, base_url: T, client: &reqwest::Client) -> Result<reqwest::Request, ApiError> {
        let method = Method::from_str(self.method.as_str());
        if method.is_err() {
            return Err(ApiError::InvalidRequestMethod(self.method.clone(), self))
        }

        let mut path = self.path.clone();
        for (key, value) in self.path_parameters.iter() {
            path = path.replace(format!("{{{}}})", key).as_str(), value);
        }

        let mut url = base_url.to_string() + path.as_str();
        for (idx, (key, value)) in self.query.iter().enumerate() {
            if idx == 0 {
                url = url + "?";
            } else {
                url = url + "&";
            }

            url = url + urlencoding::encode(key.as_str()).as_ref() + "=" + urlencoding::encode(value.as_str()).as_ref()
        }

        let mut builder = match self.builder_async {
            None => client.request(method.unwrap(), url),
            Some(bld) => bld.0(client, method.unwrap(), url),
        };

        builder = match self.entity {
            Some(ApiRequestEntity::String(s)) => builder.body(s),
            Some(ApiRequestEntity::Buffer(s)) => builder.body(s),
            #[cfg(not(target_arch = "wasm32"))]
            Some(ApiRequestEntity::Stream(s)) => builder.body(reqwest::Body::wrap_stream(tokio_util::codec::FramedRead::new(s, tokio_util::codec::BytesCodec::new()))),
            #[cfg(target_arch = "wasm32")]
            Some(ApiRequestEntity::Stream(s)) => {
                builder.body(reqwest::Body::from(s.remaining_data().map_err(|e| ApiError::Other(Box::new(e)))?))
            },
            None => builder,
        };

        builder = builder.headers(self.headers);

        builder.build().map_err(|e| ApiError::ReqwestError(e))
    }

    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn build_blocking<T: ToString>(self, base_url: T, client: &reqwest::blocking::Client) -> Result<reqwest::blocking::Request, ApiError> {
        let method = Method::from_str(self.method.as_str());
        if method.is_err() {
            return Err(ApiError::InvalidRequestMethod(self.method.clone(), self))
        }

        let mut path = self.path.clone();
        for (key, value) in self.path_parameters.iter() {
            path = path.replace(format!("{{{}}})", key).as_str(), value);
        }

        let mut url = base_url.to_string() + path.as_str();
        for (idx, (key, value)) in self.query.iter().enumerate() {
            if idx == 0 {
                url = url + "?";
            } else {
                url = url + "&";
            }

            url = url + urlencoding::encode(key.as_str()).as_ref() + "=" + urlencoding::encode(value.as_str()).as_ref()
        }

        let mut builder = match self.builder_blocking {
            None => client.request(method.unwrap(), url),
            Some(bld) => bld.0(client, method.unwrap(), url),
        };

        builder = match self.entity {
            Some(ApiRequestEntity::String(s)) => builder.body(s),
            Some(ApiRequestEntity::Buffer(s)) => builder.body(s),
            Some(ApiRequestEntity::Stream(s)) => builder.body(reqwest::blocking::Body::new(s)),
            None => builder,
        };

        builder = builder.headers(self.headers);

        builder.build().map_err(|e| ApiError::ReqwestError(e))
    }
}

#[derive(Debug, Clone, Default)]
pub struct DefaultCustomizer;

impl AnyRef for DefaultCustomizer {
    fn as_any_ref(&self) -> Box<&dyn Any> {
        Box::new(self)
    }

    fn as_any_ref_mut(&mut self) -> Box<&mut dyn Any> {
        Box::new(self)
    }
}

impl ResponseCustomizer for DefaultCustomizer {

    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_response_blocking(&self, _operation_id: &str, _client: &reqwest::blocking::Client, _request_url: &reqwest::Url, _request_headers: &HeaderMap, response: reqwest::blocking::Response)
        -> Result<either::Either<reqwest::blocking::Response, Box<dyn Any+Send>>, ApiError> {
        Ok(either::Either::Left(response))
    }

    #[cfg(feature = "async")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_response_async(&self, _operation_id: &str, _client: &reqwest::Client, _request_url: &reqwest::Url, _request_headers: &HeaderMap, response: reqwest::Response)
        -> std::pin::Pin<Box<dyn Future<Output = either::Either<reqwest::Response, Result<Box<dyn Any+Send>, ApiError>>> + Send>> {
        Box::pin(async move {
            either::Either::Left(response)
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn customize_response_async(&self, _operation_id: &str, _client: &reqwest::Client, _request_url: &reqwest::Url, _request_headers: &HeaderMap, response: reqwest::Response)
        -> std::pin::Pin<Box<dyn Future<Output = either::Either<reqwest::Response, Result<Box<dyn Any+Send>, ApiError>>>>> {
        Box::pin(async move {
            either::Either::Left(response)
        })
    }

    fn clone_to_box(&self) -> Box<dyn ResponseCustomizer> {
        Box::new(DefaultCustomizer::default())
    }
}
impl RequestCustomizer for DefaultCustomizer {

    #[cfg(feature = "blocking")]
    #[cfg(not(target_arch = "wasm32"))]
    fn customize_request_blocking(&self, _operation_id: &str, _client: &reqwest::blocking::Client, request: ApiRequestBuilder) -> Result<ApiRequestBuilder, ApiError> {
        Ok(request)
    }

    #[cfg(any(feature = "async", target_arch = "wasm32"))]
    fn customize_request_async(&self, _operation_id: &str, _client: &reqwest::Client, request: ApiRequestBuilder) -> Result<ApiRequestBuilder, ApiError> {
        Ok(request)
    }

    fn clone_to_box(&self) -> Box<dyn RequestCustomizer> {
        Box::new(DefaultCustomizer::default())
    }
}