pub mod utils;

use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::TryFutureExt;
use tokio::sync::{watch, Mutex, MutexGuard, RwLock, TryLockError};
use ton_api::ton::ton::blockidext::BlockIdExt;
use ton_api::{ton, Function, IntoBoxed};
use ton_block::{AccountState, MsgAddrStd};
pub use tonlib_sys::errors::*;
use tonlib_sys::AsQuery;

use crate::utils::*;

#[derive(Debug, Clone)]
pub struct Config {
    pub network_config: String,
    pub network_name: String,
    pub verbosity: u8,
    pub keystore: KeystoreType,
    pub last_block_threshold: Duration,
}

#[derive(Debug, Clone)]
pub enum KeystoreType {
    InMemory,
    FileSystem(String),
}

impl From<KeystoreType> for ton::KeyStoreType {
    fn from(v: KeystoreType) -> Self {
        match v {
            KeystoreType::InMemory => ton::KeyStoreType::KeyStoreTypeInMemory,
            KeystoreType::FileSystem(directory) => ton::keystoretype::KeyStoreTypeDirectory { directory }.into_boxed(),
        }
    }
}

struct LastBlock {
    id: Mutex<Option<(TonlibResult<ton::ton::blockidext::BlockIdExt>, Instant)>>,
    threshold: Duration,
}

impl LastBlock {
    fn new(threshold: &Duration) -> Self {
        Self {
            id: Mutex::new(None),
            threshold: threshold.clone(),
        }
    }

    async fn get_last_block(&self, client: &TonlibClient) -> TonlibResult<ton::ton::blockidext::BlockIdExt> {
        let mut lock = self.id.lock().await;
        let now = Instant::now();

        let new_id = match &mut *lock {
            Some((result, last)) if now.duration_since(*last) >= self.threshold => {
                return result.clone();
            }
            _ => client
                .run(&ton::rpc::lite_server::GetMasterchainInfo)
                .await
                .map(|result| result.only().last.only()),
        };

        *lock = Some((new_id.clone(), now));
        new_id
    }
}

pub struct TonlibClient {
    client: tonlib_sys::TonlibClient,
    last_block: LastBlock,
}

impl TonlibClient {
    pub async fn new(config: &Config) -> TonlibResult<Self> {
        let _ = tonlib_sys::TonlibClient::execute(
            &ton::rpc::SetLogVerbosityLevel {
                new_verbosity_level: config.verbosity as i32,
            }
            .into_query()?,
        )?;

        let client = TonlibClient {
            client: tonlib_sys::TonlibClient::new(),
            last_block: LastBlock::new(&config.last_block_threshold),
        };
        client
            .run(&ton::rpc::Init {
                options: ton::options::Options {
                    config: ton::config::Config {
                        config: config.network_config.to_string(),
                        blockchain_name: config.network_name.to_string(),
                        use_callbacks_for_network: false.into(),
                        ignore_cache: true.into(),
                    },
                    keystore_type: config.keystore.clone().into(),
                },
            })
            .await?;

        Ok(client)
    }

    pub async fn get_account_state(
        &self,
        account: ton::lite_server::accountid::AccountId,
    ) -> TonlibResult<ton::lite_server::rawaccount::RawAccount> {
        let last_block = self.last_block.get_last_block(self).await?;

        let query = ton::rpc::lite_server::GetRawAccount { id: last_block, account };

        Ok(self.run(&query).await?.only())
    }

    async fn run<T>(&self, f: &T) -> TonlibResult<T::Reply>
    where
        T: Function,
    {
        let mut result = MaybeUninit::uninit();
        TonlibFuture {
            client: &self.client,
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
        let this = self.get_mut();

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
    use std::str::FromStr;

    use super::*;
    use ton_block::MsgAddress;

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

    const ELECTOR_ADDR: &str = "-1:3333333333333333333333333333333333333333333333333333333333333333";

    fn elector_addr() -> ton::lite_server::accountid::AccountId {
        utils::make_address_from_str(ELECTOR_ADDR).unwrap()
    }

    async fn make_client() -> TonlibClient {
        std::fs::create_dir_all("./keystore").unwrap();

        TonlibClient::new(&Config {
            network_config: MAINNET_CONFIG.to_string(),
            network_name: "mainnet".to_string(),
            verbosity: 4,
            keystore: KeystoreType::InMemory,
            last_block_threshold: Duration::from_secs(1),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_init() {
        let client = make_client().await;

        let account_state = client.get_account_state(elector_addr()).await.unwrap();
    }
}
