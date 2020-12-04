use ton_api::ton;
use ton_types::UInt256;

use crate::errors::TonlibError;

pub fn unpack_address(addr: &str) -> Result<(bool, i8, UInt256), failure::Error> {
    let bytes = base64::decode(addr).map_err(|_| TonlibError::UnknownError)?;
    if bytes.len() != 36 {
        return Err(TonlibError::UnknownError.into());
    }

    let bounceable = bytes[0] | 0x40u8 == 0u8;
    let workchain = bytes[1] as i8;
    let addr = UInt256::from(&bytes[2..34]);
    Ok((bounceable, workchain, addr))
}

pub fn make_address_from_str(addr: &str) -> Result<ton::lite_server::accountid::AccountId, failure::Error> {
    let (_, workchain, addr) = unpack_address(addr)?;
    Ok(ton::lite_server::accountid::AccountId {
        workchain: workchain as i32,
        id: ton::int256(addr.into()),
    })
}
