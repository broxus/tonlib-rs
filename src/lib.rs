pub mod errors;
pub mod utils;

use std::convert::TryFrom;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use adnl::client::{AdnlClient, AdnlClientConfig};
use tokio::sync::Mutex;
use ton_api::{ton, Function};
use ton_block::{
    Account, AccountStuff, Deserializable, HashmapAugType, InRefValue, MsgAddrStd, MsgAddressInt, ShardStateUnsplit, Transaction,
    Transactions,
};
use ton_types::{HashmapType, Result, UInt256, UsageTree};

use crate::errors::*;

pub struct TonlibClient {
    client: Mutex<AdnlClient>,
    last_block: LastBlock,
}

impl TonlibClient {
    pub async fn new(config: &Config) -> Result<Self> {
        let adnl_config = AdnlClientConfig::try_from(config)?;
        let client = AdnlClient::connect(&adnl_config).await?;

        Ok(TonlibClient {
            client: Mutex::new(client),
            last_block: LastBlock::new(&config.last_block_threshold),
        })
    }

    pub async fn ping(&self) -> Result<u64> {
        self.client.lock().await.ping().await
    }

    pub async fn get_account_state<T>(&self, account: &T) -> Result<(AccountStats, AccountStuff)>
    where
        T: AsStdAddr,
    {
        let id = self.last_block.get_last_block(self).await?;
        let query = ton::rpc::lite_server::GetAccountState {
            id,
            account: ton::lite_server::accountid::AccountId {
                workchain: account.workchain_id(),
                id: ton::int256(account.address().into()),
            },
        };

        let mut response = self.run(query).await?.only();
        match Account::construct_from_bytes(&mut response.state.0)? {
            Account::Account(info) => {
                let state_root = ton_types::deserialize_tree_of_cells(&mut std::io::Cursor::new(&response.state.0))?;
                let usage_tree = UsageTree::with_root(state_root);
                let ss = ShardStateUnsplit::construct_from(&mut usage_tree.root_slice())?;

                let shard_info = ss
                    .read_accounts()?
                    .get(&account.address())?
                    .ok_or_else(|| TonlibError::AccountNotFound)?;

                Ok((
                    AccountStats {
                        last_trans_lt: shard_info.last_trans_lt(),
                        last_trans_hash: shard_info.last_trans_hash().clone(),
                        gen_lt: ss.gen_lt(),
                        gen_utime: ss.gen_time(),
                    },
                    info,
                ))
            }
            _ => Err(TonlibError::AccountNotFound.into()),
        }
    }

    pub async fn get_transactions<T>(&self, account: &T, count: u8, lt: u64, hash: UInt256) -> Result<Vec<Transaction>>
    where
        T: AsStdAddr,
    {
        let query = ton::rpc::lite_server::GetTransactions {
            count: count as i32,
            account: ton::lite_server::accountid::AccountId {
                workchain: account.workchain_id(),
                id: ton::int256(account.address().into()),
            },
            lt: lt as i64,
            hash: ton::int256(hash.into()),
        };
        let transactions = self
            .run(query)
            .await
            .and_then(|result| Transactions::construct_from_bytes(result.transactions()))?;

        let mut result = Vec::with_capacity(count as usize);
        for data in transactions.iter() {
            let transaction = InRefValue::<Transaction>::construct_from(&mut data?.1)?;
            result.push(transaction.inner());
        }
        Ok(result)
    }

    pub async fn send_message(&self, data: Vec<u8>) -> Result<()> {
        let _ = self.run(ton::rpc::lite_server::SendMessage { body: data.into() }).await?;
        Ok(())
    }

    async fn run<T>(&self, f: T) -> Result<T::Reply>
    where
        T: Function,
    {
        let query = ton::TLObject::new(f);

        let mut client = self.client.lock().await;
        let result = client.query(&query).await?;

        match result.downcast::<T::Reply>() {
            Ok(reply) => Ok(reply),
            Err(error) => match error.downcast::<ton::Error>() {
                Ok(error) => Err(TonlibError::ExecutionError {
                    code: *error.code(),
                    message: error.message().to_string(),
                }
                .into()),
                Err(_unknown) => Err(TonlibError::UnknownError.into()),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccountStats {
    pub last_trans_lt: u64,
    pub last_trans_hash: UInt256,
    pub gen_lt: u64,
    pub gen_utime: u32,
}

pub trait AsStdAddr {
    fn workchain_id(&self) -> i32;
    fn address(&self) -> UInt256;
}

impl AsStdAddr for MsgAddrStd {
    fn workchain_id(&self) -> i32 {
        self.workchain_id as i32
    }

    fn address(&self) -> UInt256 {
        self.address.get_bytestring(0).into()
    }
}

impl AsStdAddr for MsgAddressInt {
    fn workchain_id(&self) -> i32 {
        self.get_workchain_id()
    }

    fn address(&self) -> UInt256 {
        self.get_address().get_bytestring(0).into()
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub server_address: SocketAddr,
    pub server_key: String,
    pub last_block_threshold: Duration,
}

impl TryFrom<&Config> for AdnlClientConfig {
    type Error = failure::Error;

    fn try_from(value: &Config) -> Result<Self> {
        let json = serde_json::json!({
            "client_key": serde_json::Value::Null,
            "server_address": value.server_address.to_string(),
            "server_key": {
                "type_id": adnl::common::KeyOption::KEY_ED25519,
                "pub_key": value.server_key.clone(),
                "pvt_key": serde_json::Value::Null,
            },
            "timeouts": adnl::common::Timeouts::default()
        });
        AdnlClientConfig::from_json(&json.to_string())
    }
}

struct LastBlock {
    id: Mutex<Option<(TonlibResult<ton::ton_node::blockidext::BlockIdExt>, Instant)>>,
    threshold: Duration,
}

impl LastBlock {
    fn new(threshold: &Duration) -> Self {
        Self {
            id: Mutex::new(None),
            threshold: threshold.clone(),
        }
    }

    async fn get_last_block(&self, client: &TonlibClient) -> TonlibResult<ton::ton_node::blockidext::BlockIdExt> {
        let mut lock = self.id.lock().await;
        let now = Instant::now();

        let new_id = match &mut *lock {
            Some((result, last)) if now.duration_since(*last) < self.threshold => {
                return result.clone();
            }
            _ => client
                .run(ton::rpc::lite_server::GetMasterchainInfo)
                .await
                .map(|result| result.only().last)
                .map_err(|e| TonlibError::AdnlError(e.to_string())),
        };

        *lock = Some((new_id.clone(), now));
        new_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::str::FromStr;

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

    fn elector_addr() -> MsgAddressInt {
        MsgAddressInt::from_str("-1:3333333333333333333333333333333333333333333333333333333333333333").unwrap()
    }

    async fn make_client() -> TonlibClient {
        TonlibClient::new(&Config {
            server_address: "54.158.97.195:3031".parse().unwrap(),
            server_key: "uNRRL+6enQjuiZ/s6Z+vO7yxUUR7uxdfzIy+RxkECrc=".to_owned(),
            last_block_threshold: Duration::from_secs(1),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_transactions() {
        std::env::set_var("RUST_LOG", "trace");
        env_logger::init();

        let client = make_client().await;
        //
        // let ping_result = client.ping().await.unwrap();
        // println!("Ping: {}", ping_result);

        let (stats, account_state) = client.get_account_state(&elector_addr()).await.unwrap();
        println!("Account state: {:?}, {:?}", stats, account_state);

        let account_state = client
            .get_transactions(&elector_addr(), 16, stats.last_trans_lt, stats.last_trans_hash)
            .await
            .unwrap();
        println!("Transactions: {:?}", account_state);
    }
}
