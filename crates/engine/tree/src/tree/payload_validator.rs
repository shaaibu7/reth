//! Concrete implementation of the `PayloadValidator` trait.

use crate::tree::{
    payload_processor::PayloadProcessor,
    precompile_cache::{CachedPrecompileMetrics, PrecompileCacheMap},
    EngineApiTreeState, TreeConfig,
};
use alloy_rpc_types_engine::ExecutionData;
use reth_consensus::{ConsensusError, FullConsensus};
use reth_engine_primitives::{PayloadValidationOutcome, PayloadValidator, TreeCtx};
use reth_evm::{ConfigureEvm, SpecFor};
use reth_payload_primitives::NewPayloadError;
use reth_primitives_traits::{AlloyBlockHeader, NodePrimitives, RecoveredBlock, SealedHeader};
use reth_provider::{
    BlockReader, ChainSpecProvider, DatabaseProviderFactory, HeaderProvider,
    StateCommitmentProvider, StateProviderFactory, StateReader,
};
use std::{collections::HashMap, marker::Unpin, sync::Arc};
use tracing::{debug, trace};

/// A concrete implementation of the `PayloadValidator` trait that handles validation,
/// execution, and state root computation.
#[derive(Debug)]
#[allow(dead_code)]
pub struct TreePayloadValidator<N, P, C>
where
    N: NodePrimitives,
    P: BlockReader
        + StateProviderFactory
        + StateReader
        + StateCommitmentProvider
        + Clone
        + Unpin
        + 'static,
    C: ConfigureEvm<Primitives = N> + 'static,
{
    /// Provider for database access.
    provider: P,
    /// Consensus implementation for validation.
    consensus: Arc<dyn FullConsensus<N, Error = ConsensusError>>,
    /// EVM configuration.
    evm_config: C,
    /// Configuration for the tree.
    config: TreeConfig,
    /// Payload processor for state root computation.
    payload_processor: PayloadProcessor<N, C>,
    /// Precompile cache map.
    precompile_cache_map: PrecompileCacheMap<SpecFor<C>>,
    /// Precompile cache metrics.
    precompile_cache_metrics: HashMap<alloy_primitives::Address, CachedPrecompileMetrics>,
}

impl<N, P, C> TreePayloadValidator<N, P, C>
where
    N: NodePrimitives,
    P: BlockReader
        + StateProviderFactory
        + StateReader
        + StateCommitmentProvider
        + Clone
        + Unpin
        + 'static,
    C: ConfigureEvm<Primitives = N> + 'static,
{
    /// Creates a new `TreePayloadValidator`.
    pub fn new(
        provider: P,
        consensus: Arc<dyn FullConsensus<N, Error = ConsensusError>>,
        evm_config: C,
        config: TreeConfig,
        payload_processor: PayloadProcessor<N, C>,
        precompile_cache_map: PrecompileCacheMap<SpecFor<C>>,
    ) -> Self {
        Self {
            provider,
            consensus,
            evm_config,
            config,
            payload_processor,
            precompile_cache_map,
            precompile_cache_metrics: HashMap::new(),
        }
    }
}

impl<N, P, C> PayloadValidator<EngineApiTreeState<N>> for TreePayloadValidator<N, P, C>
where
    N: NodePrimitives,
    P: DatabaseProviderFactory<Provider: BlockReader>
        + BlockReader
        + StateProviderFactory
        + StateReader
        + StateCommitmentProvider
        + ChainSpecProvider
        + HeaderProvider<Header = N::BlockHeader>
        + Clone
        + Unpin
        + 'static,
    C: ConfigureEvm<Primitives = N> + 'static,
{
    type Block = N::Block;
    type ExecutionData = ExecutionData;

    fn ensure_well_formed_payload(
        &self,
        _payload: Self::ExecutionData,
    ) -> Result<RecoveredBlock<Self::Block>, NewPayloadError> {
        Err(NewPayloadError::Other("Not implemented yet".to_string().into()))
    }

    fn validate_payload(
        &self,
        payload: Self::ExecutionData,
        _ctx: TreeCtx<'_, EngineApiTreeState<N>>,
    ) -> Result<PayloadValidationOutcome<Self::Block>, NewPayloadError> {
        match self.ensure_well_formed_payload(payload) {
            Ok(block) => Ok(PayloadValidationOutcome::Valid { block }),
            Err(error) => Err(error),
        }
    }

    fn validate_block(
        &self,
        block: &RecoveredBlock<Self::Block>,
        ctx: TreeCtx<'_, EngineApiTreeState<N>>,
    ) -> Result<(), ConsensusError> {
        let block_num_hash = block.num_hash();
        debug!(target: "engine::tree", block=?block_num_hash, parent = ?block.header().parent_hash(), "Validating downloaded block");

        // Validate block consensus rules
        trace!(target: "engine::tree", block=?block_num_hash, "Validating block header");
        self.consensus.validate_header(block.sealed_header())?;

        trace!(target: "engine::tree", block=?block_num_hash, "Validating block pre-execution");
        self.consensus.validate_block_pre_execution(block)?;

        // Get parent header for validation
        let parent_hash = block.header().parent_hash();
        let parent_header =
            if let Some(parent_block) = ctx.state.tree_state.executed_block_by_hash(parent_hash) {
                parent_block.block.recovered_block.sealed_header().clone()
            } else {
                // Fallback to database if not in tree state
                let header: N::BlockHeader = self
                    .provider
                    .header(&parent_hash)
                    .map_err(|e| ConsensusError::Other(e.to_string()))?
                    .ok_or_else(|| {
                        ConsensusError::Other(format!("Parent header not found: {parent_hash}"))
                    })?;
                SealedHeader::seal_slow(header)
            };

        // Validate against parent
        trace!(target: "engine::tree", block=?block_num_hash, "Validating block against parent");
        self.consensus.validate_header_against_parent(block.sealed_header(), &parent_header)?;

        debug!(target: "engine::tree", block=?block_num_hash, "Block validation complete");
        Ok(())
    }
}
