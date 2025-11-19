use rusqlite::{Connection, Row};

pub const SYNC_BLOCK_FILE: &str = "./last_sync_block.txt";
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
    pub tx_id: String,
    pub tx_kind: String,
    pub sender: String,
    pub block_num: u32,
    pub note_id: Option<NoteData>,
    pub timestamp: u32,
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
        if row.get::<usize, String>(6).unwrap() != "NULL" {
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
        .map_err(|err| format!("failed to open db {}", err))?;
    let mut rows = stmt
        .query_map(&[(":tx_id", &tx_id)], |row| {
            Ok(Transaction::from_sql_row(row))
        })
        .map_err(|err| format!("Transaction Not Found {}", err))?;

    let res = match rows.next() {
        Some(res) => res.map_err(|err| err.to_string()),
        None => Err("No transaction foundd".to_string()),
    };
    if rows.count() > 0 {
        return Err("Failed".to_string());
    };
    res
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
        return Err("Failed to get transactions".to_string());
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
        return Err("Failed to get transactions".to_string());
    };
    Ok(res)
}
