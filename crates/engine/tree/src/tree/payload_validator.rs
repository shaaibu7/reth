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
use reth_primitives_traits::{NodePrimitives, RecoveredBlock};
use reth_provider::{
    BlockReader, ChainSpecProvider, DatabaseProviderFactory, StateCommitmentProvider,
    StateProviderFactory, StateReader,
};
use std::{collections::HashMap, marker::Unpin, sync::Arc};

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
        _block: &RecoveredBlock<Self::Block>,
        _ctx: TreeCtx<'_, EngineApiTreeState<N>>,
    ) -> Result<(), ConsensusError> {
        Ok(())
    }
}
