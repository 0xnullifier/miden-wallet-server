use std::{collections::BTreeSet, time::Duration};

use miden_client::{
    account::{AccountId, AccountIdAddress, Address, AddressInterface},
    rpc::{Endpoint, NodeRpcClient, TonicRpcClient, domain::sync::StateSyncInfo},
};
use miden_objects::account::NetworkId;
use rusqlite::{Connection, Row};

use crate::{
    server::{FAUCET_ID, STATS_FILE},
    utils::legacy_accountid_to_bech32,
};

const SYNC_BLOCK_FILE: &str = "./last_sync_block.txt";
/// Creates a worker that polls raw blocks from the rpc and see if there are changes
/// made for the rpc

#[derive(serde::Serialize, Debug)]
pub struct NoteData {
    pub note_id: String,
    pub note_type: String, // Public Private
    pub note_aux: String,
}

#[derive(serde::Serialize, Debug)]
pub struct Transaction {
    tx_id: String,
    tx_kind: String,
    sender: String,
    block_num: u32,
    note_id: Option<NoteData>,
    timestamp: u32,
}

impl Transaction {
    pub fn into_sql_value(
        self,
    ) -> (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) {
        let (note_id, note_type, note_aux) = self.note_id.map_or(
            ("NULL".to_string(), "NULL".to_string(), "NULL".to_string()),
            |val| (val.note_id, val.note_type, val.note_aux),
        );
        (
            self.block_num.to_string(),
            self.tx_id,
            self.tx_kind,
            self.sender,
            self.timestamp.to_string(),
            note_id,
            note_type,
            note_aux,
        )
    }

    pub fn from_sql_row(row: &Row) -> Self {
        let mut note_data = None;
        if row.get::<usize, String>(6).unwrap() != "NULL".to_string() {
            note_data = Some(NoteData {
                note_id: row.get(6).unwrap(),
                note_type: row.get(7).unwrap(),
                note_aux: row.get(8).unwrap(),
            })
        }

        Self {
            block_num: row.get(1).unwrap(),
            tx_id: row.get(2).unwrap(),
            tx_kind: row.get(3).unwrap(),
            sender: row.get(4).unwrap(),
            timestamp: row.get(5).unwrap(),
            note_id: note_data,
        }
    }
}

pub fn get_tx_by_id(conn: &Connection, tx_id: String) -> Result<Transaction, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM TRANSACTIONS_DETAIL WHERE tx_id = :tx_id ")
        .map_err(|err| format!("failed to open db {}", err.to_string()))?;
    let mut rows = stmt
        .query_map(&[(":tx_id", &tx_id)], |row| {
            Ok(Transaction::from_sql_row(row))
        })
        .map_err(|err| format!("Transaction Not Found {}", err.to_string()))?;
    let res = rows
        .next()
        .unwrap()
        .map_err(|err| format!("error {}", err))?;
    if rows.count() > 0 {
        return Err(format!("Failed"));
    };
    Ok(res)
}

pub fn get_txs_in_last_hour(conn: &Connection) -> Result<u32, String> {
    let mut stmt = conn
        .prepare(
            "SELECT COUNT(*) FROM TRANSACTIONS_DETAIL WHERE datetime(timestamp, 'unixepoch') >= datetime('now', '-1 hour')",
        )
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let mut rows = stmt
        .query_map([], |row| row.get::<usize, u32>(0))
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let res = rows
        .next()
        .unwrap()
        .map_err(|err| format!("Error getting transactions {}", err))?;
    println!("Transactions in last hour: {}", res);
    // If there are more than 0 rows, it means there are transactions in the last
    if rows.count() > 0 {
        return Err(format!("Failed to get transactions"));
    };
    Ok(res)
}

pub fn get_txs_latest(conn: &Connection) -> Result<Vec<Transaction>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM TRANSACTIONS_DETAIL ORDER BY id DESC LIMIT 10")
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let rows = stmt
        .query_map([], |row| Ok(Transaction::from_sql_row(row)))
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let mut res = vec![];
    for row in rows {
        res.push(row.map_err(|err| format!("Error getting transactions {}", err))?);
    }
    Ok(res)
}

// assumes account_id is a valid bech32 encoded account id
pub fn get_transactions_by_account(
    conn: &Connection,
    account_id: &str,
    page_number: u32,
) -> Result<Vec<Transaction>, String> {
    if page_number < 1 {
        return Err("Page number must be greater than 0".to_string());
    }
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM TRANSACTIONS_DETAIL WHERE sender = :account_id ORDER BY id DESC LIMIT 10 OFFSET {}", (page_number - 1) * 10))
        .map_err(|err| format!("Failed to get transactions {}", err))?;

    let rows = stmt
        .query_map(&[(":account_id", account_id)], |row| {
            Ok(Transaction::from_sql_row(row))
        })
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let mut res = vec![];
    for row in rows {
        res.push(row.map_err(|err| format!("Error getting transactions {}", err))?);
    }
    Ok(res)
}

pub fn get_number_of_tx_for_address(conn: &Connection, account_id: &str) -> Result<u32, String> {
    let mut stmt = conn
        .prepare("SELECT COUNT(*) FROM TRANSACTIONS_DETAIL WHERE sender = :account_id")
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let mut rows = stmt
        .query_map(&[(":account_id", account_id)], |row| {
            row.get::<usize, u32>(0)
        })
        .map_err(|err| format!("Failed to get transactions {}", err))?;
    let res = rows
        .next()
        .unwrap()
        .map_err(|err| format!("Error getting transactions {}", err))?;
    if rows.count() > 0 {
        return Err(format!("Failed to get transactions"));
    };
    Ok(res)
}

pub fn update_db(conn: &Connection, sync_info: &StateSyncInfo) {
    if sync_info.transactions.is_empty() {
        return;
    }
    let mut stmt = conn
        .prepare("INSERT INTO TRANSACTIONS_DETAIL (block_num, tx_id, tx_kind, sender, timestamp, note_id, note_type, note_aux) VALUES (?1, ?2,  ?3, ?4, ?5, ?6, ?7, ?8)")
        .expect("Unable to prepare statement");
    // Read previous totals from file
    let (mut total_txs, mut total_notes_created, mut total_faucet_requests) =
        std::fs::read_to_string(STATS_FILE)
            .ok()
            .and_then(|s| {
                let parts: Vec<&str> = s.trim().split(',').collect();
                if parts.len() == 3 {
                    Some((
                        parts[0].parse::<usize>().unwrap_or(0),
                        parts[1].parse::<usize>().unwrap_or(0),
                        parts[2].parse::<usize>().unwrap_or(0),
                    ))
                } else {
                    None
                }
            })
            .unwrap_or((0, 0, 0));

    total_txs += sync_info.transactions.len();
    total_notes_created += sync_info.note_inclusions.len();

    for tx in sync_info.transactions.iter() {
        let tx_id = tx.transaction_id.to_hex();
        let sender = tx.account_id;
        let found_note = sync_info
            .note_inclusions
            .iter()
            .find(|note| note.metadata().sender() == sender);

        let tx_kind = if sender == *FAUCET_ID {
            total_faucet_requests = total_faucet_requests + 1;
            "faucet_request"
        } else if found_note.is_some() {
            "send"
        } else {
            "receive"
        };
        let sender_address =
            Address::from(AccountIdAddress::new(sender, AddressInterface::BasicWallet));

        let tx = Transaction {
            tx_id,
            tx_kind: tx_kind.to_string(),
            sender: sender_address.to_bech32(NetworkId::Testnet),
            block_num: sync_info.chain_tip.as_u32(),
            note_id: found_note.map(|note| NoteData {
                note_id: note.note_id().to_hex(),
                note_type: note.metadata().note_type().to_string(),
                note_aux: note.metadata().aux().to_string(),
            }),
            timestamp: sync_info.block_header.timestamp(),
        };
        stmt.execute(tx.into_sql_value()).unwrap();
    }

    std::fs::write(
        STATS_FILE,
        format!(
            "{},{},{}",
            total_txs, total_notes_created, total_faucet_requests
        ),
    )
    .expect("Failed to write stats file");
}

pub fn get_accounts_to_be_tracked(conn: &Connection) -> Vec<AccountId> {
    let mut accounts_stmt = conn
        .prepare("SELECT * FROM ACCOUNTS")
        .expect("Query failed");
    let rows = accounts_stmt
        .query_map([], |row| row.get::<usize, String>(1))
        .expect("Query for account id failed");
    let mut accounts_to_be_tracked: Vec<AccountId> = rows
        .map(|wallet| {
            let wallet = wallet.expect("Cannot get wallet from db");
            match Address::from_bech32(&wallet) {
                Ok((_, Address::AccountId(id))) => id.id(),
                Ok((_, _)) => panic!("Address is not an AccountId"),
                Err(_) => legacy_accountid_to_bech32(&wallet)
                    .expect("Cannot convert legacy account id to bech32"),
            }
        })
        .collect();

    accounts_to_be_tracked.push(*FAUCET_ID);
    accounts_to_be_tracked
}

pub async fn start_worker() {
    let conn = Connection::open("./app_db.sqlite3").expect("Cannot open db");
    let endpoint = Endpoint::testnet();
    let rpc = TonicRpcClient::new(&endpoint, 100_000);

    let mut last_sync_block = std::fs::read_to_string(SYNC_BLOCK_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(1);

    println!("worker started");
    loop {
        let accounts_to_be_tracked = get_accounts_to_be_tracked(&conn);
        let empty_btree_set = BTreeSet::new();
        let sync_result = rpc
            .sync_state(
                last_sync_block.into(),
                &accounts_to_be_tracked,
                &empty_btree_set,
            )
            .await
            .expect("Sync failed");
        last_sync_block = sync_result.chain_tip.as_u32();
        std::fs::write(SYNC_BLOCK_FILE, last_sync_block.to_string())
            .expect("Failed to write last_sync_block");
        update_db(&conn, &sync_result);
        // a 1 second sleep to not get rate limited
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
