use bech32::{Bech32m, primitives::decode::CheckedHrpstring};
use miden_client::account::{AccountId, AddressType};

const SERIALIZED_SIZE: usize = 15;

/// Copy from earlier version of Miden Base
pub fn legacy_accountid_to_bech32(bech32_string: &str) -> Result<AccountId, String> {
    // We use CheckedHrpString with an explicit checksum algorithm so we don't allow the
    // `Bech32` or `NoChecksum` algorithms.
    let checked_string = CheckedHrpstring::new::<Bech32m>(bech32_string)
        .map_err(|source| format!("Failed to decode bech32 string: {source}"))?;

    let mut byte_iter = checked_string.byte_iter();
    // The length must be the serialized size of the account ID plus the address byte.
    if byte_iter.len() != SERIALIZED_SIZE + 1 {
        return Err(format!(
            "Invalid address length: expected {}, got {}",
            SERIALIZED_SIZE + 1,
            byte_iter.len()
        ));
    }

    let address_byte = byte_iter.next().expect("there should be at least one byte");
    if address_byte != AddressType::AccountId as u8 {
        return Err(format!(
            "Invalid address type byte: expected {}, got {}",
            AddressType::AccountId as u8,
            address_byte
        ));
    }

    // Every byte is guaranteed to be overwritten since we've checked the length of the
    // iterator.
    let mut id_bytes = [0_u8; 15];
    for (i, byte) in byte_iter.enumerate() {
        id_bytes[i] = byte;
    }

    let account_id = AccountId::try_from(id_bytes)
        .map_err(|source| format!("Failed to create AccountId from bytes: {source}"))?;

    Ok(account_id)
}
