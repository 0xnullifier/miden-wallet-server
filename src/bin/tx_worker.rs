use miden_client::{
    account::{AccountId, AccountIdAddress, Address, AddressInterface, NetworkId},
    rpc::{Endpoint, NodeRpcClient, TonicRpcClient, domain::note::FetchedNote},
};
use miden_faucet_server::{
    server::{APP_DB, FAUCET_ID},
    tx_worker::{NoteData, SYNC_BLOCK_FILE, Transaction},
    utils::legacy_accountid_to_bech32,
};
use miden_objects::block::ProvenBlock;
use rusqlite::Connection;
use std::error::Error;
use std::{collections::BTreeSet, time::Duration};

pub fn get_accounts_to_be_tracked(conn: &Connection) -> BTreeSet<AccountId> {
    let mut stmt = conn
        .prepare("SELECT * FROM ACCOUNTS")
        .expect("Unable to prepare statement");
    let account_iter = stmt
        .query_map([], |row| {
            let wallet: String = row.get(1)?;
            match Address::from_bech32(&wallet) {
                Ok((_, Address::AccountId(id))) => Ok(id.id()),
                Ok((_, _)) => panic!("Address is not an AccountId"),
                Err(_) => {
                    Ok(legacy_accountid_to_bech32(&wallet)
                        .expect("Unable to parse legacy account id"))
                }
            }
        })
        .expect("Unable to query accounts");
    let mut accounts = BTreeSet::new();
    for account in account_iter {
        accounts.insert(account.expect("Unable to get account"));
    }
    accounts.insert(*FAUCET_ID);
    accounts
}

/// returns the total txs, total notes created, total faucet requests
pub async fn update_db_raw_block(
    conn: &Connection,
    rpc: &TonicRpcClient,
    accounts_to_be_tracked: &BTreeSet<AccountId>,
    block: &ProvenBlock,
) -> Result<(), Box<dyn std::error::Error>> {
    // check if the block contains updated accounts we are tracking
    let mut stmt = conn
        .prepare("INSERT OR IGNORE INTO TRANSACTIONS_DETAIL (block_num, tx_id, tx_kind, sender, timestamp, note_id, note_type, note_aux) VALUES (?1, ?2,  ?3, ?4, ?5, ?6, ?7, ?8)")
        .expect("Unable to prepare statement");
    let txs = block.transactions().as_slice();
    for tx in txs {
        if !accounts_to_be_tracked.contains(&tx.account_id()) {
            continue;
        }
        let tx_id = tx.id().to_hex();
        let sender = tx.account_id();
        let mut found_note = None;
        let tx_kind = if sender == *FAUCET_ID {
            "faucet_request"
        } else if !tx.output_notes().is_empty() {
            let note_id = tx.output_notes()[0];
            let note: Result<FetchedNote, _> = rpc.get_note_by_id(note_id).await;
            found_note = match note {
                Ok(FetchedNote::Public(note, _)) => Some(NoteData {
                    note_id: note.id().to_hex(),
                    note_type: note.metadata().note_type().to_string(),
                    note_aux: note.metadata().aux().to_string(),
                }),
                Ok(FetchedNote::Private(note_id, note_aux, _)) => Some(NoteData {
                    note_id: note_id.to_hex(),
                    note_type: "private".to_string(),
                    note_aux: note_aux.aux().to_string(),
                }),
                Err(e) => {
                    println!(
                        "Error fetching note {} at tx {}: {}",
                        note_id.to_hex(),
                        tx_id,
                        e
                    );
                    None
                }
            };
            "send"
        } else if !tx.input_notes().is_empty() {
            "receive"
        } else {
            return Err("Unknown tx kind".into());
        };

        let sender_address =
            Address::from(AccountIdAddress::new(sender, AddressInterface::Unspecified));

        let tx = Transaction {
            tx_id,
            tx_kind: tx_kind.to_string(),
            sender: sender_address.to_bech32(NetworkId::Testnet),
            block_num: block.header().block_num().as_u32(),
            note_id: found_note,
            timestamp: block.header().timestamp(),
        };
        stmt.execute(tx.into_sql_value())?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;
    println!("WORKER STARTED");
    let conn = Connection::open(APP_DB).expect("Cannot open db");
    let rpc = TonicRpcClient::new(&Endpoint::testnet(), 100_000);
    let empty_btree_set = BTreeSet::new();

    // get the last sync block
    let mut last_sync_block = std::fs::read_to_string(SYNC_BLOCK_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1);
    loop {
        // find accounts to be tracked
        let accounts_to_be_tracked = get_accounts_to_be_tracked(&conn);
        // find the latest block
        let latest_block = rpc
            .sync_state(0.into(), &[], &empty_btree_set)
            .await?
            .chain_tip
            .as_u32();
        let mut i = last_sync_block;
        while i <= latest_block {
            if i.is_multiple_of(100) {
                println!(
                    "Progress: {:.2}%, Block: {}/{}",
                    (i as f64 / latest_block as f64) * 100.0,
                    i,
                    latest_block
                );
            }
            let raw_block = match rpc.get_block_by_number(i.into()).await {
                Ok(block) => block,
                Err(e) => {
                    panic!("Error fetching block {}: {}", i, e);
                }
            };
            let updated_accounts: Vec<AccountId> = raw_block
                .updated_accounts()
                .iter()
                .map(|acc| acc.account_id())
                .collect();
            let other: BTreeSet<AccountId> = updated_accounts.into_iter().collect();
            if accounts_to_be_tracked.is_disjoint(&other) {
                i += 1;
                continue;
            }
            update_db_raw_block(&conn, &rpc, &accounts_to_be_tracked, &raw_block).await?;
            i += 1;
            std::fs::write(SYNC_BLOCK_FILE, i.to_string())
                .expect("Failed to write last_sync_block");
        }
        last_sync_block = latest_block;
        std::fs::write(SYNC_BLOCK_FILE, last_sync_block.to_string())
            .expect("Failed to write last_sync_block");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
