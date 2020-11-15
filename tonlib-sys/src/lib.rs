pub mod errors;

use std::ffi::c_void;
use std::os::raw::c_ulong;
use ton_api::{Deserializer, Function, Serializer};

use crate::errors::*;

#[repr(C)]
struct ExecutionResult {
    data_ptr: *const c_void,
    data_len: c_ulong,
}

#[link(name = "tonlib-sys-cpp-bundled", kind = "static")]
extern "C" {
    fn trs_create_client() -> *mut c_void;
    fn trs_delete_client(client: *mut c_void);

    fn trs_run(client: *mut c_void, query_ptr: *const c_void, query_len: u64) -> ExecutionResult;
    fn trs_execute(query_ptr: *const c_void, query_len: u64) -> ExecutionResult;
    fn trs_delete_response(response: *const ExecutionResult);
}

struct ExecutionResultHandle(ExecutionResult);

impl ExecutionResultHandle {
    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.data_ptr as *const u8, self.0.data_len as usize) }
    }
}

impl Drop for ExecutionResultHandle {
    fn drop(&mut self) {
        unsafe { trs_delete_response(&self.0) };
    }
}

struct TonlibClient(*mut c_void);

impl TonlibClient {
    pub fn execute<T>(function: &T) -> TonlibResult<T::Reply>
    where
        T: Function,
    {
        let mut buf = Vec::<u8>::new();
        Serializer::new(&mut buf)
            .write_boxed(function)
            .map_err(|e| TonlibError::SerializationError { reason: e.to_string() })?;
        let result = ExecutionResultHandle(unsafe { trs_execute(buf.as_ptr() as *const c_void, buf.len() as u64) });

        let mut buf = std::io::Cursor::new(result.as_slice());
        Deserializer::new(&mut buf)
            .read_boxed()
            .map_err(|e| TonlibError::DeserializationError { reason: e.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use ton_api::ton;

    use super::TonlibClient;

    #[test]
    fn test_static_function() {
        let result = TonlibClient::execute(&ton::rpc::GenerateKeyPair {
            word_count: 24,
            password: ton_api::secure::SecureString::new(b"Hello world".to_vec()),
            entropy: ton_api::secure::SecureString::new(b"Entropy".to_vec()),
        });

        println!("{:?}", result);
    }
}
