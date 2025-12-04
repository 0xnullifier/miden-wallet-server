use std::{collections::BTreeSet, error::Error, sync::Arc, time::Instant};

use miden_client::{
    Client, RemoteTransactionProver,
    account::AccountId,
    address::Address,
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    note_transport::grpc::GrpcNoteTransportClient,
    rpc::{Endpoint, GrpcClient},
    store::{NoteFilter, TransactionFilter},
    sync::StateSync,
};
use miden_client_sqlite_store::SqliteStore;
use rand::rngs::StdRng;

use crate::note_screener::NoteScreener;

const TX_PROVER_ENDPOINT: &str = "https://tx-prover.testnet.miden.io";
const NOTE_TRANSPORT_URL: &str = "https://transport.miden.io";

pub fn validate_address(bech32_string: &str) -> bool {
    match Address::decode(bech32_string) {
        Ok((_, _)) => true,
        Err(_) => false,
    }
}

/// override the default sync state for client
pub async fn sync_state(
    account_id: AccountId,
    client: &mut Client<FilesystemKeyStore<StdRng>>,
    sqlite_store: Arc<SqliteStore>,
    rpc: Arc<GrpcClient>,
) -> Result<(), Box<dyn Error>> {
    let note_screener = NoteScreener::new(sqlite_store.clone());

    let state_sync_component = StateSync::new(rpc, Arc::new(note_screener), None);

    let accounts = client
        .get_account_header_by_id(account_id)
        .await?
        .map(|(header, _)| vec![header])
        .unwrap_or_default();
    let note_tags = BTreeSet::new();
    let input_notes = vec![];
    let expected_output_notes = client.get_output_notes(NoteFilter::Expected).await?;
    let uncommitted_transactions = client
        .get_transactions(TransactionFilter::Uncommitted)
        .await?;
    let current_partial_mmr = client.get_current_partial_mmr().await?;
    let state_sync_update = state_sync_component
        .sync_state(
            current_partial_mmr,
            accounts,
            note_tags,
            input_notes,
            expected_output_notes,
            uncommitted_transactions,
        )
        .await
        .expect("failed to sync state");
    client
        .apply_state_sync(state_sync_update)
        .await
        .expect("failed to apply state sync");

    Ok(())
}

pub async fn init_client(
    client_db: &str,
    endpoint: Endpoint,
) -> Client<FilesystemKeyStore<StdRng>> {
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(GrpcClient::new(&endpoint, timeout_ms));
    let sqlite_store = SqliteStore::new(client_db.into()).await.unwrap();

    let note_tranport =
        GrpcNoteTransportClient::connect(NOTE_TRANSPORT_URL.to_string(), timeout_ms)
            .await
            .unwrap();
    let remote_prover = Arc::new(RemoteTransactionProver::new(TX_PROVER_ENDPOINT.to_string()));
    ClientBuilder::new()
        .store(Arc::new(sqlite_store))
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true.into())
        .note_transport(Arc::new(note_tranport))
        .prover(remote_prover)
        .build()
        .await
        .expect("Failed to build client")
}

pub async fn init_client_with_custom_sync_state(
    client_db: &str,
    endpoint: Endpoint,
    faucet_id: AccountId,
) -> Client<FilesystemKeyStore<StdRng>> {
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(GrpcClient::new(&endpoint, timeout_ms));
    let sqlite_store = SqliteStore::new(client_db.into()).await.unwrap();

    let note_tranport =
        GrpcNoteTransportClient::connect(NOTE_TRANSPORT_URL.to_string(), timeout_ms)
            .await
            .unwrap();
    let remote_prover = Arc::new(RemoteTransactionProver::new(TX_PROVER_ENDPOINT.to_string()));
    let mut client = ClientBuilder::new()
        .store(Arc::new(sqlite_store))
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true.into())
        .note_transport(Arc::new(note_tranport))
        .prover(remote_prover)
        .build()
        .await
        .expect("Failed to build client");
    client.ensure_genesis_in_place().await.unwrap();
    let time = Instant::now();
    let rpc_api = Arc::new(GrpcClient::new(&endpoint, timeout_ms));
    let sqlite_store = SqliteStore::new(client_db.into()).await.unwrap();
    sync_state(faucet_id, &mut client, Arc::new(sqlite_store), rpc_api)
        .await
        .unwrap();
    println!("State synced in {:?}", time.elapsed());
    client
}
