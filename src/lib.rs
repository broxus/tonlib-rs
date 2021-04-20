mod errors;
pub mod utils;

use std::convert::TryFrom;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use adnl::client::{AdnlClient, AdnlClientConfig};
use tokio::sync::Mutex;
use ton_api::{ton, Function};
use ton_block::{Account, AccountStuff, Deserializable, HashmapAugType, MsgAddrStd, MsgAddressInt, ShardStateUnsplit, Transaction};
use ton_types::{Result, UInt256};

use crate::errors::*;

pub struct TonlibClient {
    adnl_config: AdnlClientConfig,
    client: Mutex<AdnlClient>,
    last_block: LastBlock,
}

impl TonlibClient {
    pub async fn new(config: &Config) -> Result<Self> {
        let adnl_config = AdnlClientConfig::try_from(config)?;
        let client = AdnlClient::connect(&adnl_config).await?;

        let tonlib = TonlibClient {
            adnl_config,
            client: Mutex::new(client),
            last_block: LastBlock::new(&config.last_block_threshold),
        };

        Ok(tonlib)
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

        let response = self.run(query).await?.only();
        if response.state.0.is_empty() {
            return Err(TonlibError::AccountNotFound.into());
        }

        match Account::construct_from_bytes(&response.state.0)? {
            Account::Account(info) => {
                let q_roots = ton_types::deserialize_cells_tree(&mut std::io::Cursor::new(&response.proof.0))?;
                if q_roots.len() != 2 {
                    return Err(TonlibError::InvalidAccountState.into());
                }

                let merkle_proof = ton_block::MerkleProof::construct_from_cell(q_roots[0].clone())?;
                let proof_root = merkle_proof.proof.virtualize(1);

                let ss = ShardStateUnsplit::construct_from(&mut proof_root.into())?;

                let shard_info = ss.read_accounts()?.get(&account.address())?.ok_or(TonlibError::AccountNotFound)?;

                Ok((
                    AccountStats {
                        last_trans_lt: shard_info.last_trans_lt(),
                        last_trans_hash: *shard_info.last_trans_hash(),
                        gen_lt: ss.gen_lt(),
                        gen_utime: ss.gen_time(),
                    },
                    info,
                ))
            }
            _ => Err(TonlibError::AccountNotFound.into()),
        }
    }

    pub async fn get_transactions<T>(&self, account: &T, count: u8, lt: u64, hash: UInt256) -> Result<Vec<(UInt256, Transaction)>>
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
        let transactions = self.run(query).await.and_then(|result| {
            let data = result.only().transactions.0;
            if data.is_empty() {
                return Ok(Vec::new());
            }

            ton_types::deserialize_cells_tree(&mut std::io::Cursor::new(data))
        })?;

        let mut result = Vec::with_capacity(transactions.len());
        for data in transactions.into_iter().rev() {
            let hash = data.repr_hash();
            result.push((hash, Transaction::construct_from_cell(data)?));
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
        let query_bytes = f.boxed_serialized_bytes()?;
        let query = ton::TLObject::new(ton::rpc::lite_server::Query { data: query_bytes.into() });

        let mut client = self.client.lock().await;
        let result = match client.query(&query).await {
            Ok(result) => result,
            Err(e) => match client.ping().await {
                Ok(_) => return Err(e),
                Err(ping_error) => {
                    log::error!("ADNL error: {:?} then unsuccessful ping: {}", e, ping_error);

                    log::warn!("Reconnecting");
                    *client = AdnlClient::connect(&self.adnl_config).await?;

                    log::warn!("Retrying request");
                    client.query(&query).await?
                }
            },
        };

        match result.downcast::<T::Reply>() {
            Ok(reply) => Ok(reply),
            Err(error) => match error.downcast::<ton::lite_server::Error>() {
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
            "client_key": serde_json::Value::Null,
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
            threshold: *threshold,
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

    use std::str::FromStr;

    use futures::future::Future;

    fn elector_addr() -> MsgAddressInt {
        MsgAddressInt::from_str("-1:3333333333333333333333333333333333333333333333333333333333333333").unwrap()
    }

    fn unknown_addr() -> MsgAddressInt {
        MsgAddressInt::from_str("-1:3333333333333333333333333333333333333333333333333333333333333334").unwrap()
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

    fn run_test<T>(fut: impl Future<Output = Result<T>>) {
        let mut rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(fut).unwrap();
    }

    #[test]
    fn test_transactions() {
        run_test(async {
            let client = make_client().await;

            let (stats, account_state) = client.get_account_state(&elector_addr()).await?;
            println!("Account state: {:?}, {:?}", stats, account_state);

            let transactions = client
                .get_transactions(&elector_addr(), 16, stats.last_trans_lt, stats.last_trans_hash)
                .await?;

            println!("Transactions: {:?}", transactions);
            Ok(())
        });
    }

    #[test]
    fn test_unknown() {
        run_test(async {
            let client = make_client().await;

            let transactions = client.get_transactions(&unknown_addr(), 16, 0, UInt256::default()).await?;
            assert!(transactions.is_empty());
            Ok(())
        });
    }
}
