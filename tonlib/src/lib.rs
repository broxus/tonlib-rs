use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::{Context, Poll};

use ton_api::{ton, Function};
pub use tonlib_sys::errors::*;
use tonlib_sys::AsQuery;

#[derive(Debug, Clone)]
pub struct Config {
    pub network_config: String,
    pub network_name: String,
    pub verbosity: u8,
}

pub struct TonlibClient(tonlib_sys::TonlibClient);

impl TonlibClient {
    pub async fn new(config: &Config) -> TonlibResult<Self> {
        let _ = tonlib_sys::TonlibClient::execute(
            &ton::rpc::SetLogVerbosityLevel {
                new_verbosity_level: config.verbosity as i32,
            }
            .into_query()?,
        )?;

        let client = TonlibClient(tonlib_sys::TonlibClient::new());
        client
            .run(&ton::rpc::Init {
                options: ton::options::Options {
                    config: ton::config::Config {
                        config: config.network_config.to_string(),
                        blockchain_name: config.network_name.to_string(),
                        use_callbacks_for_network: false.into(),
                        ignore_cache: true.into(),
                    },
                    keystore_type: ton::KeyStoreType::KeyStoreTypeInMemory,
                },
            })
            .await?;

        Ok(client)
    }

    async fn run<T>(&self, f: &T) -> TonlibResult<T::Reply>
    where
        T: Function,
    {
        let mut result = MaybeUninit::uninit();
        TonlibFuture {
            client: &self.0,
            function: Some(f),
            result: Some(&mut result),
        }
        .await;
        unsafe { result.assume_init() }
    }
}

struct TonlibFuture<'f, T>
where
    T: Function,
{
    client: &'f tonlib_sys::TonlibClient,
    function: Option<&'f T>,
    result: Option<&'f mut MaybeUninit<TonlibResult<T::Reply>>>,
}

impl<'f, T> Future for TonlibFuture<'f, T>
where
    T: Function,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        match (this.function.take(), this.result.take()) {
            (Some(f), Some(value)) => {
                let waker = cx.waker().clone();

                match f.as_query() {
                    Ok(query) => {
                        this.client.run::<T, _>(
                            &query,
                            Box::new(move |result| {
                                *value = MaybeUninit::new(result);
                                waker.wake();
                            }),
                        );
                        Poll::Pending
                    }
                    Err(e) => {
                        *value = MaybeUninit::new(Err(e));
                        Poll::Ready(())
                    }
                }
            }
            (None, None) => Poll::Ready(()),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[tokio::test]
    async fn test_init() {
        let _client = TonlibClient::new(&Config {
            network_config: MAINNET_CONFIG.to_string(),
            network_name: "mainnet".to_string(),
            verbosity: 4,
        })
        .await
        .unwrap();
    }
}
