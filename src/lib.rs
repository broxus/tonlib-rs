mod connection;
mod errors;
mod last_block;
mod pool;
pub mod utils;

use std::convert::TryFrom;
use std::net::SocketAddrV4;
use std::time::Duration;

use anyhow::Result;
use bb8::{Pool, PooledConnection};
use tiny_adnl::AdnlTcpClientConfig;
use ton_api::ton;
use ton_block::{AccountStuff, Deserializable, MsgAddrStd, MsgAddressInt, Transaction};
use ton_types::UInt256;

use crate::connection::*;
use crate::errors::*;
use crate::last_block::*;
use crate::pool::*;

pub struct TonlibClient {
    pool: Pool<AdnlManageConnection>,
    last_block: LastBlock,
}

impl TonlibClient {
    pub async fn new(config: &Config) -> Result<Self> {
        let builder = Pool::builder();
        let pool = builder
            .max_size(config.max_connection_count)
            .min_idle(config.min_idle_connection_count)
            .max_lifetime(None)
            .build(AdnlManageConnection::new(config)?)
            .await?;

        Ok(Self {
            pool,
            last_block: LastBlock::new(&config.last_block_threshold),
        })
    }

    pub async fn get_account_state<T>(&self, account: &T) -> TonlibResult<(AccountStats, AccountStuff)>
    where
        T: AsStdAddr,
    {
        use ton_block::HashmapAugType;

        let mut connection = self.acquire_connection().await?;
        let last_block_id = self.last_block.get_last_block(&mut connection).await?;

        let mut account_state_query = ton::rpc::lite_server::GetAccountState {
            id: last_block_id.clone(),
            account: ton::lite_server::accountid::AccountId {
                workchain: account.workchain_id(),
                id: ton::int256(account.address().into()),
            },
        };

        let response = {
            match query(&mut connection, &account_state_query).await? {
                QueryReply::Data(data) => data,
                QueryReply::NotReady => {
                    let previous_block_ids = self
                        .last_block
                        .last_cached_blocks()
                        .await
                        .skip_while(|block| block.seqno < last_block_id.seqno);

                    let mut result = QueryReply::NotReady;
                    for block_id in previous_block_ids {
                        account_state_query.id = block_id;
                        result = query(&mut connection, &account_state_query).await?;

                        if result.has_data() {
                            break;
                        }
                    }

                    result.try_into_data()?
                }
            }
        }
        .only();

        match ton_block::Account::construct_from_bytes(&response.state.0) {
            Ok(ton_block::Account::Account(info)) => {
                let q_roots = ton_types::deserialize_cells_tree(&mut std::io::Cursor::new(&response.proof.0))
                    .map_err(|_| TonlibError::InvalidAccountStateProof)?;
                if q_roots.len() != 2 {
                    return Err(TonlibError::InvalidAccountStateProof);
                }

                let merkle_proof =
                    ton_block::MerkleProof::construct_from_cell(q_roots[1].clone()).map_err(|_| TonlibError::InvalidAccountStateProof)?;
                let proof_root = merkle_proof.proof.virtualize(1);

                let ss = ton_block::ShardStateUnsplit::construct_from(&mut proof_root.into())
                    .map_err(|_| TonlibError::InvalidAccountStateProof)?;

                let shard_info = ss
                    .read_accounts()
                    .and_then(|accounts| accounts.get(&account.address()))
                    .map_err(|_| TonlibError::InvalidAccountStateProof)?
                    .ok_or(TonlibError::AccountNotFound)?;

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
            _ => Err(TonlibError::AccountNotFound),
        }
    }

    pub async fn get_transactions<T>(&self, account: &T, count: u8, lt: u64, hash: UInt256) -> Result<Vec<(UInt256, Transaction)>>
    where
        T: AsStdAddr,
    {
        let mut connection = self.acquire_connection().await?;

        let response = query(
            &mut connection,
            &ton::rpc::lite_server::GetTransactions {
                count: count as i32,
                account: ton::lite_server::accountid::AccountId {
                    workchain: account.workchain_id(),
                    id: ton::int256(account.address().into()),
                },
                lt: lt as i64,
                hash: ton::int256(hash.into()),
            },
        )
        .await?
        .try_into_data()?;

        let transactions = response.only().transactions.0;
        if transactions.is_empty() {
            return Ok(Vec::new());
        }

        let transactions = ton_types::deserialize_cells_tree(&mut std::io::Cursor::new(transactions)).map_err(anyhow::Error::msg)?;

        let mut result = Vec::with_capacity(transactions.len());
        for data in transactions.into_iter().rev() {
            let hash = data.repr_hash();
            result.push((hash, Transaction::construct_from_cell(data).map_err(anyhow::Error::msg)?));
        }
        Ok(result)
    }

    pub async fn send_message(&self, data: Vec<u8>) -> Result<()> {
        let mut connection = self.acquire_connection().await?;

        query(&mut connection, &ton::rpc::lite_server::SendMessage { body: ton::bytes(data) })
            .await?
            .try_into_data()?;
        Ok(())
    }

    async fn acquire_connection(&self) -> TonlibResult<PooledConnection<'_, AdnlManageConnection>> {
        acquire_connection(&self.pool).await
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
    pub server_address: SocketAddrV4,
    pub server_key: String,
    pub max_connection_count: u32,
    pub min_idle_connection_count: Option<u32>,
    pub socket_read_timeout: Duration,
    pub socket_send_timeout: Duration,
    pub last_block_threshold: Duration,
    pub ping_timeout: Duration,
}

impl TryFrom<&Config> for AdnlTcpClientConfig {
    type Error = anyhow::Error;

    fn try_from(c: &Config) -> Result<Self> {
        let server_key = base64::decode(&c.server_key)?;

        Ok(AdnlTcpClientConfig {
            server_address: c.server_address,
            server_key: ed25519_dalek::PublicKey::from_bytes(&server_key)?,
            socket_read_timeout: c.socket_read_timeout,
            socket_send_timeout: c.socket_send_timeout,
        })
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
            max_connection_count: 1,
            min_idle_connection_count: Some(1),
            socket_read_timeout: Duration::from_secs(5),
            socket_send_timeout: Duration::from_secs(5),
            ping_timeout: Duration::from_secs(10),
            last_block_threshold: Duration::from_secs(1),
        })
        .await
        .unwrap()
    }

    fn run_test<T>(fut: impl Future<Output = Result<T>>) {
        let rt = tokio::runtime::Runtime::new().unwrap();
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
