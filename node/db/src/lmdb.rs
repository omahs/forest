// Copyright 2019-2022 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use super::errors::Error;
use crate::utils::bitswap_missing_blocks;
use crate::{DBStatistics, Store};
use cid::Cid;
use fvm_ipld_blockstore::Blockstore;
use libp2p_bitswap::BitswapStore;
use lmdb::{Database, Environment, Transaction};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct LMDb {
    pub env: Arc<Environment>,
    pub db: Arc<Database>,
}

pub struct LMDbConfig {
    pub path: PathBuf,
}

impl LMDbConfig {
    pub fn from_path(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl LMDb {
    fn to_options(config: &LMDbConfig) -> Result<Environment, Error> {
        let env_builder = Environment::new();
        env_builder.open(&config.path).map_err(Error::from)
    }

    pub fn open(config: &LMDbConfig) -> anyhow::Result<Self> {
        let env = Self::to_options(config)?;
        let db = env.open_db(None)?;
        Ok(Self {
            env: Arc::new(env),
            db: Arc::new(db),
        })
    }
}

impl Store for LMDb {
    fn read<K>(&self, key: K) -> Result<Option<Vec<u8>>, Error>
    where
        K: AsRef<[u8]>,
    {
        let rtxn = self.env.begin_ro_txn()?;
        Ok(Some(
            lmdb::Transaction::get(&rtxn, *self.db, &key)?.to_vec(),
        ))
    }

    fn write<K, V>(&self, key: K, value: V) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let mut rwtxn = self.env.begin_rw_txn()?;
        rwtxn.put(*self.db, &key, &value, lmdb::WriteFlags::empty())?;
        Transaction::commit(rwtxn).map_err(Error::from)
    }

    fn delete<K>(&self, key: K) -> Result<(), Error>
    where
        K: AsRef<[u8]>,
    {
        let mut rwtxn = self.env.begin_rw_txn()?;
        rwtxn.del(*self.db, &key, None)?;
        Transaction::commit(rwtxn).map_err(Error::from)
    }

    fn exists<K>(&self, key: K) -> Result<bool, Error>
    where
        K: AsRef<[u8]>,
    {
        let rtxn = self.env.begin_ro_txn()?;
        Ok(lmdb::Transaction::get(&rtxn, *self.db, &key).ok().is_some())
    }
}

impl Blockstore for LMDb {
    fn get(&self, k: &Cid) -> anyhow::Result<Option<Vec<u8>>> {
        self.read(k.to_bytes()).map_err(|e| e.into())
    }

    fn put_keyed(&self, k: &Cid, block: &[u8]) -> anyhow::Result<()> {
        self.write(k.to_bytes(), block).map_err(|e| e.into())
    }

    fn put_many_keyed<D, I>(&self, blocks: I) -> anyhow::Result<()>
    where
        Self: Sized,
        D: AsRef<[u8]>,
        I: IntoIterator<Item = (Cid, D)>,
    {
        let values = blocks
            .into_iter()
            .map(|(k, v)| (k.to_bytes(), v))
            .collect::<Vec<_>>();
        self.bulk_write(&values).map_err(|e| e.into())
    }
}

impl BitswapStore for LMDb {
    /// `fvm_ipld_encoding::DAG_CBOR(0x71)` is covered by [`libipld::DefaultParams`]
    /// under feature `dag-cbor`
    type Params = libipld::DefaultParams;

    fn contains(&mut self, cid: &Cid) -> anyhow::Result<bool> {
        Ok(self.exists(cid.to_bytes())?)
    }

    fn get(&mut self, cid: &Cid) -> anyhow::Result<Option<Vec<u8>>> {
        Blockstore::get(self, cid)
    }

    fn insert(&mut self, block: &libipld::Block<Self::Params>) -> anyhow::Result<()> {
        self.put_keyed(block.cid(), block.data())
    }

    fn missing_blocks(&mut self, cid: &Cid) -> anyhow::Result<Vec<Cid>> {
        bitswap_missing_blocks::<_, Self::Params>(self, cid)
    }
}

impl DBStatistics for LMDb {}
