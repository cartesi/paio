use std::io::{self, Read, Write};
use std::result::Result;

use message::Batch;

fn to_json_string(str: &str) -> Result<String, Box<dyn std::error::Error>> {
    let batch = Batch::from_bytes(&alloy_core::hex::decode(str.as_bytes())?)?;
    Ok(serde_json::to_string(&batch)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin = io::stdin();
    let mut buffer = String::new();
    stdin.read_to_string(&mut buffer)?;

    let j = to_json_string(buffer.trim_end())?;

    let mut stdout = io::stdout();
    stdout.write_all(j.as_bytes())?;
    stdout.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloy_core::primitives::{address, U256};
    use message::{
        WireTransaction, WireSignature, WireParity,
        PARITY_TYPE_EIP155, PARITY_TYPE_NON_EIP155, PARITY_TYPE_PARITY
    };

    use super::*;

    #[test]
    fn test() {
        let batch = Batch{
            sequencer_payment_address: address!("63F9725f107358c9115BC9d86c72dD5823E9B1E6"),
            txs: vec![
                WireTransaction {
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 1,
                    max_gas_price: 10,
                    data: vec![71,101,108,108,111,44,32,87,111,114,108,100,61],
                    signature: WireSignature{
                        r: U256::default(),
                        s: U256::default(),
                        v: WireParity{
                            parity_type: PARITY_TYPE_EIP155,
                            eip155_value: 100,
                            noneip155_value: false,
                            pairty_value: false,
                        }
                    },
                },
                WireTransaction {
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 2,
                    max_gas_price: 20,
                    data: vec![72,101,108,108,111,44,32,87,111,114,108,100,62],
                    signature: WireSignature{
                        r: U256::default(),
                        s: U256::default(),
                        v: WireParity{
                            parity_type: PARITY_TYPE_NON_EIP155,
                            eip155_value: 0,
                            noneip155_value: true,
                            pairty_value: false,
                        }
                    },
                },
                WireTransaction {
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 2,
                    max_gas_price: 20,
                    data: vec![73,101,108,108,111,44,32,87,111,114,108,100,63],
                    signature: WireSignature{
                        r: U256::default(),
                        s: U256::default(),
                        v: WireParity{
                            parity_type: PARITY_TYPE_PARITY,
                            eip155_value: 0,
                            noneip155_value: false,
                            pairty_value: true,
                        }
                    },
                },
            ],
        };

        let encoded = alloy_core::hex::encode(batch.to_bytes());
        let decoded_json = to_json_string(&encoded).unwrap();
        let decoded: Batch = serde_json::from_str(&decoded_json).unwrap();

        assert_eq!(batch.sequencer_payment_address, decoded.sequencer_payment_address);
        assert_eq!(batch.txs, decoded.txs);
    }
}
