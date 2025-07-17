//! Concrete implementation of the `PayloadValidator` trait.

use crate::tree::{
    payload_processor::PayloadProcessor,
    precompile_cache::{CachedPrecompileMetrics, PrecompileCacheMap},
    EngineApiTreeState, TreeConfig,
};
use alloy_eips::eip2718::Decodable2718;
use alloy_rpc_types_engine::ExecutionData;
use reth_consensus::{ConsensusError, FullConsensus};
use reth_engine_primitives::{PayloadValidationOutcome, PayloadValidator, TreeCtx};
use reth_evm::{ConfigureEvm, SpecFor};
use reth_payload_primitives::NewPayloadError;
use reth_primitives_traits::{
    AlloyBlockHeader, Block, BlockBody, NodePrimitives, RecoveredBlock, SealedHeader,
};
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

    /// Converts an execution payload to a recovered block.
    fn payload_to_block(
        &self,
        payload: ExecutionData,
    ) -> Result<RecoveredBlock<N::Block>, NewPayloadError>
    where
        N::Block: Block<Body: BlockBody<Transaction = N::SignedTx>>
            + From<alloy_consensus::Block<N::SignedTx>>,
        N::SignedTx: Decodable2718,
    {
        let ExecutionData { payload, sidecar } = payload;

        let expected_hash = payload.block_hash();

        // Parse the block from the payload
        let alloy_block: alloy_consensus::Block<N::SignedTx> =
            payload.try_into_block_with_sidecar(&sidecar).map_err(NewPayloadError::Eth)?;

        // Convert to N::Block
        let block: N::Block = alloy_block.into();

        // Seal the block
        let sealed_block = block.seal_slow();

        // Ensure the hash included in the payload matches the block hash
        if expected_hash != sealed_block.hash() {
            return Err(NewPayloadError::Eth(alloy_rpc_types_engine::PayloadError::BlockHash {
                execution: sealed_block.hash(),
                consensus: expected_hash,
            }));
        }

        // Recover senders for the block
        let recovered_block = sealed_block
            .try_recover()
            .map_err(|_| NewPayloadError::Other("Failed to recover senders".to_string().into()))?;

        Ok(recovered_block)
    }
}

impl<N, P, C> PayloadValidator<EngineApiTreeState<N>> for TreePayloadValidator<N, P, C>
where
    N: NodePrimitives,
    N::Block: Block<Body: BlockBody<Transaction = N::SignedTx>>
        + From<alloy_consensus::Block<N::SignedTx>>,
    N::SignedTx: Decodable2718,
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
        ctx: TreeCtx<'_, EngineApiTreeState<N>>,
    ) -> Result<PayloadValidationOutcome<Self::Block>, NewPayloadError> {
        // First, convert the payload to a block
        let block = self.payload_to_block(payload)?;

        // Then validate the block using the validate_block method
        match self.validate_block(&block, ctx) {
            Ok(()) => Ok(PayloadValidationOutcome::Valid { block }),
            Err(error) => {
                // Convert consensus error to payload error
                let payload_error = NewPayloadError::Other(Box::new(error));
                Ok(PayloadValidationOutcome::Invalid { block, error: payload_error })
            }
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
