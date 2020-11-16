pub mod errors;

use std::ffi::c_void;
use std::os::raw::c_ulong;

use ton_api::{BoxedDeserialize, Deserializer, Function, Serializer};

use crate::errors::*;

pub struct TonlibClient(*mut c_void);

impl TonlibClient {
    pub fn new() -> Self {
        Self(unsafe { trs_create_client() })
    }

    pub fn run<T, F>(&self, query: &Query<T>, cb: F)
    where
        T: Function,
        F: FnOnce(TonlibResult<T::Reply>) + Send + Sync,
    {
        let (callback, context) = make_callback(Box::new(move |res: ExecutionResult| cb(ExecutionResultHandle(res).parse())));
        unsafe { trs_run(self.0, query.0.as_ptr() as *const c_void, query.0.len() as u64, callback, context) };
    }

    pub fn execute<T>(query: &Query<T>) -> TonlibResult<T::Reply>
    where
        T: Function,
    {
        ExecutionResultHandle(unsafe { trs_execute(query.0.as_ptr() as *const c_void, query.0.len() as u64) }).parse()
    }
}

impl Drop for TonlibClient {
    fn drop(&mut self) {
        unsafe { trs_delete_client(self.0) }
    }
}

pub struct Query<T>(Vec<u8>, std::marker::PhantomData<T>);

impl<T> Query<T>
where
    T: Function,
{
    pub fn new(function: &T) -> TonlibResult<Self> {
        let mut buf = Vec::<u8>::new();
        Serializer::new(&mut buf)
            .write_boxed(function)
            .map_err(|e| TonlibError::SerializationError { reason: e.to_string() })?;

        Ok(Self(buf, Default::default()))
    }
}

pub trait AsQuery {
    type Function;

    fn as_query(&self) -> TonlibResult<Query<Self::Function>>;
    fn into_query(self) -> TonlibResult<Query<Self::Function>>;
}

impl<T> AsQuery for T
where
    T: Function,
{
    type Function = T;

    fn as_query(&self) -> TonlibResult<Query<Self::Function>> {
        Query::new(self)
    }

    fn into_query(self) -> TonlibResult<Query<Self::Function>> {
        Query::new(&self)
    }
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

fn make_callback<A, F: FnOnce(A)>(f: Box<F>) -> (unsafe extern "C" fn(*mut c_void, A), *mut c_void)
where
    F: Send + Sync,
{
    let ptr = Box::into_raw(f);
    unsafe extern "C" fn callback<A, F: FnOnce(A)>(data: *mut c_void, arg: A) {
        Box::from_raw(data as *mut F)(arg)
    }
    (callback::<A, F>, ptr as *mut c_void)
}

#[cfg(test)]
mod tests {
    use ton_api::ton;

    use super::*;

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
    fn test_static_function() {
        let result = TonlibClient::execute(
            &ton::rpc::UnpackAccountAddress {
                account_address: "-1:3333333333333333333333333333333333333333333333333333333333333333".to_string(),
            }
            .as_query()
            .unwrap(),
        )
        .unwrap();

        println!("{:?}", result);
    }

    #[test]
    fn test_client() {
        let _ = TonlibClient::execute(&ton::rpc::SetLogVerbosityLevel { new_verbosity_level: 7 }.as_query().unwrap()).unwrap();

        let client = TonlibClient::new();

        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        client.run(
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
            }
            .as_query()
            .unwrap(),
            move |res| {
                println!("Result: {:?}", res);
                tx.send(res).unwrap();
            },
        );

        let _ = rx.recv().unwrap();
    }
}
