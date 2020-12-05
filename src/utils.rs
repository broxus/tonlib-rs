use ton_types::UInt256;

use crate::errors::TonlibError;

pub fn unpack_address(addr: &str) -> Result<(bool, i8, UInt256), failure::Error> {
    let bytes = base64::decode(addr).map_err(|_| TonlibError::InvalidAddress)?;
    if bytes.len() != 36 {
        return Err(TonlibError::InvalidAddress.into());
    }

    let bounceable = (bytes[0] & 0x40u8) == 0u8;
    let workchain = bytes[1] as i8;
    let addr = UInt256::from(&bytes[2..34]);
    Ok((bounceable, workchain, addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elector_addr() -> UInt256 {
        UInt256::from([
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, //
            0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
        ])
    }

    #[test]
    fn unpack_bounceable() {
        let addr = "Ef8zMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzM0vF";
        let (bounceable, workchain, addr) = unpack_address(addr).unwrap();
        assert!(bounceable);
        assert_eq!(workchain, -1);
        assert_eq!(addr, elector_addr());
    }

    #[test]
    fn unpack_non_bounceable() {
        let addr = "Uf8zMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMxYA";
        let (bounceable, workchain, addr) = unpack_address(addr).unwrap();
        assert!(!bounceable);
        assert_eq!(workchain, -1);
        assert_eq!(addr, elector_addr());
    }
}
