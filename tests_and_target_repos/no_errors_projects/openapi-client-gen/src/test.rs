use std::alloc::{alloc, dealloc, Layout};
use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::future::Future;
use std::io::Read;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};
use reqwest::{Client, Url};
use tokio::io::AsyncRead;
use crate::rt::{DefaultCustomizer, ResponseCustomizer, StringArray};


#[cfg(feature = "ffi")]
static MAP: LazyLock<Mutex<HashMap<usize, Layout>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
#[cfg(feature = "ffi")]
extern "C" fn alloc_data(size: usize, align: usize) -> *mut c_void {
    let mut g = MAP.lock().unwrap();
    unsafe {
        let layout = Layout::from_size_align(size, align).unwrap();
        let ptr = alloc(layout);
        g.insert(ptr as usize, layout);
        ptr.cast()
    }
}

#[cfg(feature = "ffi")]
fn alloc_free(ptr: *mut c_void) {
    let mut g = MAP.lock().unwrap();
    let p = ptr.cast();
    let layout = g.remove(&(p as usize)).unwrap();
    unsafe {
        dealloc(p, layout);
    }
}

#[cfg(feature = "ffi")]
#[test]
pub fn test_StringArray_copy_contiguous() {
    let mut array = StringArray::default();
    array.push("hello".into());
    array.push("my".into());
    array.push("".into());
    array.push(None.into());
    array.push("good".into());
    array.push("friend".into());
    array.push("\0".into());
    unsafe {
        let res = crate::rt::StringArray_copy_contiguous(&array, 0, array.len(), Some(alloc_data));
        let r = res.as_ref().unwrap();
        assert_eq!(r.count, array.len());
        let sl = std::slice::from_raw_parts(r.strings, array.len());
        for i in 0..array.len() {
            if array[i].is_none() {
                assert!(sl[i].is_null());
                continue;
            }
            let data = CStr::from_ptr(sl[i]).to_str().unwrap().to_string();
            let expect =  array[i].as_ref().unwrap();
            if expect == "\0" {
                assert_eq!(data.as_str(), " ");
                continue;
            }
            assert_eq!(data.as_str(), expect);
        }

        alloc_free(res.cast());
    }

}

#[cfg(feature = "ffi")]
#[test]
pub fn test_StringArray_copy() {
    let mut array = StringArray::default();
    array.push("hello".into());
    array.push("my".into());
    array.push("".into());
    array.push(None.into());
    array.push("good".into());
    array.push("friend".into());
    array.push("\0".into());
    unsafe {
        let res =  crate::rt::StringArray_copy(&array, 0, array.len(), Some(alloc_data));
        let r = res.as_ref().unwrap();
        assert_eq!(r.count, array.len());
        let sl = std::slice::from_raw_parts(r.strings, array.len());
        for i in 0..array.len() {
            if array[i].is_none() {
                assert!(sl[i].is_null());
                continue;
            }
            let data = CStr::from_ptr(sl[i]).to_str().unwrap().to_string();
            let expect =  array[i].as_ref().unwrap();
            if expect == "\0" {
                assert_eq!(data.as_str(), " ");
                alloc_free(sl[i].cast_mut().cast());
                continue;
            }
            assert_eq!(data.as_str(), expect);
            alloc_free(sl[i].cast_mut().cast());
        }

        alloc_free(res.cast());
    }
}

#[cfg(feature = "async")]
#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
#[test]
pub fn test_blocking_async_read() {
    if !crate::rt::has_tokio_runtime() {
        crate::rt::set_tokio_runtime(tokio::runtime::Builder::new_current_thread().build().unwrap());
    }

    let data = vec![0u8, 1u8, 2u8, 3u8];
    let base_stream = crate::rt::Stream::from(data.clone());
    let async_box: Box<dyn AsyncRead+Send+Unpin> = Box::new(base_stream) as Box<dyn AsyncRead+Send+Unpin>;
    let mut async_box_stream = crate::rt::Stream::from(async_box);
    let mut data2 = Vec::new();
    let cnt = std::io::Read::read_to_end(&mut async_box_stream, &mut data2).unwrap();
    assert_eq!(cnt, 4);
    assert_eq!(data, data2);
}

#[cfg(feature = "async")]
#[cfg(feature = "blocking")]
#[cfg(not(target_arch = "wasm32"))]
#[test]
pub fn test_blocking_async_read2() {
    if !crate::rt::has_tokio_runtime() {
        crate::rt::set_tokio_runtime(tokio::runtime::Builder::new_current_thread().build().unwrap());
    }

    let data = vec![0u8, 1u8, 2u8, 3u8];
    let base_stream = crate::rt::Stream::from(data.clone());
    let async_box1: Box<dyn Read + Send> = Box::new(base_stream) as Box<dyn Read + Send>;
    let mut async_box_stream1 = crate::rt::Stream::from(async_box1);
    let async_box2: Box<dyn AsyncRead + Send + Unpin> = Box::new(async_box_stream1) as Box<dyn AsyncRead + Send + Unpin>;
    let mut async_box_stream2 = crate::rt::Stream::from(async_box2);
    let mut data2 = Vec::new();

    let cnt = std::io::Read::read_to_end(&mut async_box_stream2, &mut data2).unwrap();
    assert_eq!(cnt, 4);
    assert_eq!(data, data2);
}