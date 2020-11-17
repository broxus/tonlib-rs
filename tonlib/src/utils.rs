use ton_api::ton;
use tonlib_sys::errors::TonlibResult;
use tonlib_sys::AsQuery;

pub fn unpack_address(addr: &str) -> TonlibResult<ton::unpackedaccountaddress::UnpackedAccountAddress> {
    tonlib_sys::TonlibClient::execute(
        &ton::rpc::UnpackAccountAddress {
            account_address: addr.to_string(),
        }
        .into_query()?,
    )
    .map(|addr| addr.only())
}

pub fn make_address_from_str(addr: &str) -> TonlibResult<ton::lite_server::accountid::AccountId> {
    let unpacked = unpack_address(addr)?;
    Ok(ton::lite_server::accountid::AccountId {
        workchain: unpacked.workchain_id,
        id: unpacked.addr,
    })
}
