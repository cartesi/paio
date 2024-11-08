use std::io::{self, Read, Write};
use std::result::Result;

use message::proto_message;

fn to_json_string(str: &str) -> Result<String, Box<dyn std::error::Error>> {
    let batch = proto_message::Batch
        ::from_bytes(&alloy_core::hex::decode(str.as_bytes())?)?;
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
    use alloy_core::primitives::{address, Bytes, Parity, Signature, U256};
    use message::{SignedTransaction, SigningMessage};

    use super::*;

    #[test]
    fn test() {
        let txns = vec![
            SignedTransaction{
                message: SigningMessage{
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 1,
                    max_gas_price: 10,
                    data: Bytes::from(vec![71,101,108,108,111,44,32,87,111,114,108,100,61]),
                },
                signature: Signature::new(
                    U256::from(10), 
                    U256::from(20), 
                    Parity::Eip155(30),
                ),
            },
            SignedTransaction{
                message: SigningMessage{
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 101,
                    max_gas_price: 10001,
                    data: Bytes::from(vec![72,101,108,108,111,44,32,87,111,114,108,100,62]),
                },
                signature: Signature::new(
                    U256::from(101), 
                    U256::from(202), 
                    Parity::NonEip155(true),
                ),
            },
            SignedTransaction{
                message: SigningMessage{
                    app: address!("ab7528bb862fB57E8A2BCd567a2e929a0Be56a5e"),
                    nonce: 1001,
                    max_gas_price: 10000001,
                    data: Bytes::from(vec![73,101,108,108,111,44,32,87,111,114,108,100,63]),
                },
                signature: Signature::new(
                    U256::from(1001), 
                    U256::from(2002), 
                    Parity::Parity(true),
                ),
            },
        ];

        let proto_txns: Vec<proto_message::Transaction> = txns
            .iter()
            .map(proto_message::Transaction::from_signed_transaction)
            .collect();

        let batch = proto_message::Batch{
            sequencer_payment_address: Vec::new(),
            transactions: proto_txns,
        };

        let encoded = alloy_core::hex::encode(batch.to_bytes());
        let decoded_json = to_json_string(&encoded).unwrap();
        let decoded: proto_message::Batch = serde_json::from_str(&decoded_json).unwrap();

        let decoded_txns: Vec<SignedTransaction> = decoded.transactions
            .iter()
            .map(|t| t.to_signed_transaction())
            .collect();

        assert_eq!(txns, decoded_txns);
    }
}
