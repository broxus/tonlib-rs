pub mod errors;

use std::ffi::c_void;
use std::os::raw::c_ulong;

use ton_api::{BoxedDeserialize, Deserializer, Function, Serializer};

use crate::errors::*;

#[repr(C)]
struct ExecutionResult {
    data_ptr: *const c_void,
    data_len: c_ulong,
}

type Callback = unsafe extern "C" fn(*mut c_void, ExecutionResult);

#[link(name = "tonlib-sys-cpp-bundled", kind = "static")]
extern "C" {
    fn trs_create_client() -> *mut c_void;
    fn trs_delete_client(client: *mut c_void);

    fn trs_run(client: *mut c_void, query_ptr: *const c_void, query_len: u64, callback: Callback, context: *mut c_void);
    fn trs_execute(query_ptr: *const c_void, query_len: u64) -> ExecutionResult;
    fn trs_delete_response(response: *const ExecutionResult);
}

struct ExecutionResultHandle(ExecutionResult);

impl ExecutionResultHandle {
    fn parse<T>(self) -> TonlibResult<T>
    where
        T: BoxedDeserialize,
    {
        let mut buf = std::io::Cursor::new(self.as_slice());
        Deserializer::new(&mut buf)
            .read_boxed()
            .map_err(|e| TonlibError::DeserializationError { reason: e.to_string() })
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.data_ptr as *const u8, self.0.data_len as usize) }
    }
}

impl Drop for ExecutionResultHandle {
    fn drop(&mut self) {
        unsafe { trs_delete_response(&self.0) };
    }
}

pub struct TonlibClient(*mut c_void);

impl TonlibClient {
    pub fn new() -> Self {
        Self(unsafe { trs_create_client() })
    }

    pub fn run<T, F>(&self, function: &T, mut cb: F) -> TonlibResult<()>
    where
        T: Function,
        F: FnMut(TonlibResult<T::Reply>) + Send + Sync,
    {
        let mut buf = Vec::<u8>::new();
        Serializer::new(&mut buf)
            .write_boxed(function)
            .map_err(|e| TonlibError::SerializationError { reason: e.to_string() })?;

        let (callback, context) = make_callback(Box::new(move |res: ExecutionResult| cb(ExecutionResultHandle(res).parse())));

        unsafe { trs_run(self.0, buf.as_ptr() as *const c_void, buf.len() as u64, callback, context) };

        Ok(())
    }

    pub fn execute<T>(function: &T) -> TonlibResult<T::Reply>
    where
        T: Function,
    {
        let mut buf = Vec::<u8>::new();
        Serializer::new(&mut buf)
            .write_boxed(function)
            .map_err(|e| TonlibError::SerializationError { reason: e.to_string() })?;

        ExecutionResultHandle(unsafe { trs_execute(buf.as_ptr() as *const c_void, buf.len() as u64) }).parse()
    }
}

impl Drop for TonlibClient {
    fn drop(&mut self) {
        unsafe { trs_delete_client(self.0) }
    }
}

fn make_callback<A, F: FnMut(A)>(f: Box<F>) -> (unsafe extern "C" fn(*mut c_void, A), *mut c_void)
where
    F: Send + Sync,
{
    let ptr = Box::into_raw(f);
    unsafe extern "C" fn callback<A, F: FnMut(A)>(data: *mut c_void, arg: A) {
        Box::from_raw(data as *mut F)(arg)
    }
    (callback::<A, F>, ptr as *mut c_void)
}

#[cfg(test)]
mod tests {
    use ton_api::ton;

    use super::TonlibClient;

    const MAINNET_CONFIG: &str = r#"{
      "liteservers": [
        {
          "ip": 916349379,
          "port": 3031,
          "id": {
            "@type": "pub.ed25519",
            "key": "uNRRL+6enQjuiZ/s6Z+vO7yxUUR7uxdfzIy+RxkECrc="
          }
        }
      ],
      "validator": {
        "@type": "validator.config.global",
        "zero_state": {
          "workchain": -1,
          "shard": -9223372036854775808,
          "seqno": 0,
          "root_hash": "WP/KGheNr/cF3lQhblQzyb0ufYUAcNM004mXhHq56EU=",
          "file_hash": "0nC4eylStbp9qnCq8KjDYb789NjS25L5ZA1UQwcIOOQ="
        }
      }
    }"#;

    #[test]
    fn test_client() {
        let _ = TonlibClient::execute(&ton::rpc::SetLogVerbosityLevel { new_verbosity_level: 7 }).unwrap();

        let client = TonlibClient::new();

        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        client
            .run(
                &ton::rpc::Init {
                    options: ton::options::Options {
                        config: ton::config::Config {
                            config: MAINNET_CONFIG.to_string(),
                            blockchain_name: "mainnet".to_string(),
                            use_callbacks_for_network: false.into(),
                            ignore_cache: true.into(),
                        },
                        keystore_type: ton::KeyStoreType::KeyStoreTypeInMemory,
                    },
                },
                move |res| {
                    println!("Result: {:?}", res);
                    tx.send(res).unwrap();
                },
            )
            .unwrap();

        let _ = rx.recv().unwrap();
    }

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
