//! A dummy service for testing purpose only.

use std::collections::HashMap;

use bitcoin::blockdata::constants::DIFFCHANGE_INTERVAL;
use ckb_bitcoin_spv_verifier::{
    types::{core, packed, prelude::*},
    utilities::{bitcoin::calculate_next_target, mmr},
};

use crate::result::Result;

/// A dummy service for testing the SPV client cells's bootstrap and update.
pub struct DummyService {
    client: core::SpvClient,
    store: mmr::lib::util::MemStore<packed::HeaderDigest>,
    headers: HashMap<u32, core::Header>,
}

impl DummyService {
    pub fn bootstrap(height: u32, header: core::Header) -> Result<Self> {
        if height % DIFFCHANGE_INTERVAL != 0 {
            panic!("bad height");
        }
        let mut headers = HashMap::new();
        let store = mmr::lib::util::MemStore::default();
        let client = {
            let mut mmr = mmr::ClientRootMMR::new(0, &store);
            let block_hash = header.block_hash().into();
            let digest = core::HeaderDigest::new_leaf(height, block_hash).pack();
            mmr.push(digest)?;
            let headers_mmr_root = mmr.get_root()?.unpack();
            mmr.commit()?;
            headers.insert(height, header);
            let target_adjust_info = packed::TargetAdjustInfo::encode(header.time, header.bits);
            core::SpvClient {
                id: 0,
                tip_block_hash: block_hash,
                headers_mmr_root,
                target_adjust_info,
            }
        };
        Ok(Self {
            client,
            store,
            headers,
        })
    }

    pub fn update(&mut self, header: core::Header) -> Result<packed::SpvUpdate> {
        let height = self.client.headers_mmr_root.max_height + 1;
        let positions = {
            let index = height - self.client.headers_mmr_root.min_height;
            let position = mmr::lib::leaf_index_to_pos(u64::from(index));
            vec![position]
        };
        let mut mmr = {
            let last_index =
                self.client.headers_mmr_root.max_height - self.client.headers_mmr_root.min_height;
            let mmr_size = mmr::lib::leaf_index_to_mmr_size(u64::from(last_index));
            mmr::ClientRootMMR::new(mmr_size, &self.store)
        };
        let block_hash = header.block_hash().into();
        let digest = core::HeaderDigest::new_leaf(height, block_hash).pack();
        mmr.push(digest)?;

        match (height + 1) % DIFFCHANGE_INTERVAL {
            0 => {
                let curr_target: core::Target = header.bits.into();
                log::trace!(
                    ">>> height {height:07}, time: {}, target {curr_target:#x}",
                    header.time
                );
                let start_time: u32 = self.client.target_adjust_info.start_time().unpack();
                let next_target = calculate_next_target(curr_target, start_time, header.time);
                log::info!(">>> calculated new target  {next_target:#x}");
                let next_bits = next_target.to_compact_lossy();
                let next_target: core::Target = next_bits.into();
                log::info!(">>> after definition lossy {next_target:#x}");

                self.client.target_adjust_info =
                    packed::TargetAdjustInfo::encode(start_time, next_bits);
            }
            1 => {
                self.client.target_adjust_info =
                    packed::TargetAdjustInfo::encode(header.time, header.bits);
            }
            _ => {}
        };

        self.client.tip_block_hash = block_hash;
        self.client.headers_mmr_root.max_height = height;
        self.client.headers_mmr_root = mmr.get_root()?.unpack();
        self.headers.insert(height, header);

        let headers_mmr_proof_items = mmr
            .gen_proof(positions)?
            .proof_items()
            .iter()
            .map(Clone::clone)
            .collect::<Vec<_>>();
        mmr.commit()?;
        let headers_mmr_proof = packed::MmrProof::new_builder()
            .set(headers_mmr_proof_items)
            .build();
        Ok(packed::SpvUpdate::new_builder()
            .headers(vec![header].pack())
            .new_headers_mmr_proof(headers_mmr_proof)
            .build())
    }

    pub fn tip_client(&self) -> core::SpvClient {
        self.client.clone()
    }

    pub fn min_height(&self) -> u32 {
        self.client.headers_mmr_root.min_height
    }

    pub fn max_height(&self) -> u32 {
        self.client.headers_mmr_root.max_height
    }

    pub fn generate_header_proof(&self, height: u32) -> Result<Option<core::MmrProof>> {
        if height < self.client.headers_mmr_root.min_height
            || self.client.headers_mmr_root.max_height < height
        {
            return Ok(None);
        }
        let index = height - self.client.headers_mmr_root.min_height;
        let position = mmr::lib::leaf_index_to_pos(u64::from(index));
        let last_index =
            self.client.headers_mmr_root.max_height - self.client.headers_mmr_root.min_height;
        let mmr_size = mmr::lib::leaf_index_to_mmr_size(u64::from(last_index));
        let mmr = mmr::ClientRootMMR::new(mmr_size, &self.store);
        let proof = mmr
            .gen_proof(vec![position])?
            .proof_items()
            .iter()
            .map(|item| item.unpack())
            .collect::<Vec<_>>();
        Ok(Some(proof))
    }
}
