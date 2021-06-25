use bb8::{Pool, PooledConnection};
use ton_api::ton;

use super::errors::*;
use crate::pool::AdnlManageConnection;

pub async fn query<T>(connection: &mut PooledConnection<'_, AdnlManageConnection>, query: &T) -> TonlibResult<QueryReply<T::Reply>>
where
    T: ton_api::Function,
{
    const MAX_RETIRES: usize = 3;
    const RETRY_INTERVAL: u64 = 100; // Milliseconds

    const ERR_NOT_READY: i32 = 651;

    let query_bytes = query.boxed_serialized_bytes().map_err(|_| TonlibError::FailedToSerialize)?;

    let query = ton::TLObject::new(ton::rpc::lite_server::Query { data: query_bytes.into() });

    let mut retries = 0;
    loop {
        let response = connection.query(&query).await.map_err(|_| TonlibError::ConnectionError)?;

        match response.downcast::<T::Reply>() {
            Ok(reply) => return Ok(QueryReply::Data(reply)),
            Err(error) => match error.downcast::<ton::lite_server::Error>() {
                Ok(error) if error.code() == &ERR_NOT_READY => {
                    if retries < MAX_RETIRES {
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_INTERVAL)).await;
                        retries += 1;
                        continue;
                    } else {
                        return Ok(QueryReply::NotReady);
                    }
                }
                Ok(error) => return Err(TonlibError::LiteServer(error)),
                Err(_) => return Err(TonlibError::Unknown),
            },
        }
    }
}

pub async fn acquire_connection(pool: &Pool<AdnlManageConnection>) -> TonlibResult<PooledConnection<'_, AdnlManageConnection>> {
    pool.get().await.map_err(|e| {
        log::error!("connection error: {:#?}", e);
        TonlibError::ConnectionError
    })
}

pub enum QueryReply<T> {
    Data(T),
    NotReady,
}

impl<T> QueryReply<T> {
    pub fn has_data(&self) -> bool {
        match self {
            Self::Data(_) => true,
            Self::NotReady => false,
        }
    }

    pub fn try_into_data(self) -> TonlibResult<T> {
        match self {
            Self::Data(data) => Ok(data),
            Self::NotReady => Err(TonlibError::NotReady),
        }
    }
}
