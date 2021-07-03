use std::convert::TryFrom;
use std::ops::DerefMut;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use bb8::PooledConnection;
use tiny_adnl::{AdnlTcpClient, AdnlTcpClientConfig};

use crate::Config;

pub struct AdnlManageConnection {
    config: AdnlTcpClientConfig,
    ping_timeout: Duration,
}

impl AdnlManageConnection {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: AdnlTcpClientConfig::try_from(config)?,
            ping_timeout: config.ping_timeout,
        })
    }
}

#[async_trait]
impl bb8::ManageConnection for AdnlManageConnection {
    type Connection = Arc<AdnlTcpClient>;
    type Error = anyhow::Error;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        log::debug!("Establishing adnl connection...");
        match AdnlTcpClient::connect(self.config.clone()).await {
            Ok(connection) => {
                log::debug!("Established adnl connection");
                Ok(connection)
            }
            Err(e) => {
                log::debug!("Failed to establish adnl connection");
                Err(e)
            }
        }
    }

    async fn is_valid(&self, conn: &mut PooledConnection<'_, Self>) -> Result<(), Self::Error> {
        log::trace!("Check if connection is valid...");
        match conn.deref_mut().ping(self.ping_timeout).await {
            Ok(_) => {
                log::trace!("Connection is valid");
                Ok(())
            }
            Err(e) => {
                log::trace!("Connection is invalid");
                Err(e)
            }
        }
    }

    fn has_broken(&self, connection: &mut Self::Connection) -> bool {
        connection.has_broken.load(Ordering::Acquire)
    }
}
