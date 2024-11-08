use std::{collections::HashMap, ops::Add};

use anyhow::{Error, anyhow};

use alloy_core::{
    primitives::{aliases::U120, Address, Parity, SignatureError, U160, U256},
    sol,
    sol_types::{eip712_domain, Eip712Domain, SolStruct, SolValue},
};
use alloy_signer::Signature;

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use derive_more::{Display, Into};
use prost::Message;
use serde::{Deserialize, Serialize};

pub mod proto_message;
pub struct WalletState {
    pub domain: Eip712Domain,

    // app address to app state
    pub app_nonces: HashMap<Address, AppNonces>,

    // user address to balance
    pub balances: HashMap<Address, U256>,
}

impl WalletState {
    pub fn verify_batch(&mut self, batch: proto_message::Batch) -> Vec<Transaction> {
        batch
            .transactions
            .iter()
            .filter_map(|tx| {
                let address = address_from_bytes(&batch.sequencer_payment_address);
                self.verify_single(address, tx)
            })
            .collect()
    }
    // TODO: create custom error type in order to explain why it did not work
    pub fn verify_single(
        &mut self,
        sequencer_payment_address: Address,
        tx: &proto_message::Transaction,
    ) -> Option<Transaction> {
        let app_nonce = self.app_nonces
            .entry(address_from_bytes(&tx.app))
            .or_default();
        let tx_opt = app_nonce.verify_tx(tx, &self.domain);

        if let Some(ref tx) = tx_opt {
            let cost_opt = tx.cost();
            let payment = if let Some(cost) = cost_opt {
                self.withdraw_forced(tx.sender, cost)
            } else {
                self.withdraw_forced(tx.sender, U256::MAX)
            };
            self.deposit(sequencer_payment_address, payment);
        }

        tx_opt
    }

    pub fn verify_raw_batch(&mut self, raw_batch: &[u8]) -> Result<Vec<Transaction>, Error> {
        let batch = proto_message::Batch::from_bytes(raw_batch)?;
        Ok(self.verify_batch(batch))
    }

    pub fn deposit(&mut self, user: Address, value: U256) {
        let balance = self.balances.entry(user).or_default();
        *balance += value;
    }

    pub fn withdraw_forced(&mut self, user: Address, value: U256) -> U256 {
        let balance = self.balances.entry(user).or_default();
        if *balance < value {
            let prev = *balance;
            *balance = U256::ZERO;
            prev
        } else {
            *balance -= value;
            value
        }
    }
}

impl Default for WalletState {
    fn default() -> Self {
        WalletState {
            domain: DOMAIN.clone(),
            app_nonces: HashMap::new(),
            balances: HashMap::new(),
        }
    }
}

impl WalletState {
    pub fn add_app_nonce(&mut self, address: Address, nonces: AppNonces) {
        self.app_nonces.insert(address, nonces);
    }
}

pub struct AppState {
    pub domain: Eip712Domain,
    pub address: Address,
    pub nonces: AppNonces,
}

impl AppState {
    pub fn verify_batch(&mut self, batch: proto_message::Batch) -> Vec<Transaction> {
        batch
            .transactions
            .iter()
            .filter_map(|tx| {
                if self.address != address_from_bytes(&tx.app) {
                    return None;
                }

                self.nonces.verify_tx(tx, &self.domain)
            })
            .collect()
    }

    pub fn verify_raw_batch(&mut self, raw_batch: &[u8]) -> Result<Vec<Transaction>, Error> {
        let batch = proto_message::Batch::from_bytes(raw_batch)?;
        Ok(self.verify_batch(batch))
    }
}

#[derive(Default)]
pub struct AppNonces {
    // user address to nonce
    pub nonces: HashMap<Address, u64>,
}

impl AppNonces {
    pub fn new() -> Self {
        AppNonces {
            nonces: HashMap::new(),
        }
    }
    pub fn set_nonce(&mut self, address: Address, value: u64) {
        self.nonces.insert(address, value);
    }
    pub fn get_nonce(&self, address: &Address) -> Option<&u64> {
        self.nonces.get(address)
    }
    pub fn verify_tx(
        &mut self,
        tx: &proto_message::Transaction,
        domain: &Eip712Domain,
    ) -> Option<Transaction> {
        tracing::info!("verifying tx under domain ..");
        let tx = tx.verify(domain)?;
        tracing::info!("verifying ok!");
        let expected_nonce = self.nonces.entry(tx.sender).or_insert(0);

        if *expected_nonce != tx.nonce {
            tracing::error!("verify failed: expected nonce {:?} got {:?}", *expected_nonce, tx.nonce);
            return None;
        }

        *expected_nonce += 1;
        Some(tx)
    }
}

pub struct Transaction {
    pub sender: Address,
    pub app: Address,
    pub nonce: u64,
    pub max_gas_price: u128,

    pub data: Vec<u8>,
}

impl Transaction {
    pub fn cost(&self) -> Option<U256> {
        U256::checked_mul(U256::from(self.max_gas_price), U256::from(self.data.len()))
    }
}

sol! {
   #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct SigningMessage {
        address app;
        uint64 nonce;
        uint128 max_gas_price;
        bytes data;
    }
}

impl proto_message::Batch {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode_to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<proto_message::Batch, Error> {
        match proto_message::Batch::decode(bytes) {
            Ok(b) => Ok(b),
            Err(err) => Err(anyhow!("#{}", err))
        }
    }
}

impl proto_message::Transaction {
    pub fn from_signed_transaction(value: &SignedTransaction) -> Self {
        Self {
            app: value.message.app.to_vec(),
            nonce: value.message.nonce,
            max_gas_price: u256_to_bytes(U256::from(value.message.max_gas_price)),
            data: value.message.data.to_vec(),
            signature: Some(proto_message::Signature::from_signature(&value.signature)),
        }
    }

     pub fn to_signed_transaction(&self) -> SignedTransaction {
        SignedTransaction {
            message: SigningMessage {
                app: address_from_bytes(&self.app),
                nonce: self.nonce,
                max_gas_price: u128_from_bytes(&self.max_gas_price),
                data: self.data.clone().into(),
            },
            signature: self.signature.clone().unwrap().to_signature(),
        }
    }

    pub fn verify(&self, domain: &Eip712Domain) -> Option<Transaction> {
        let signed_tx = self.to_signed_transaction();
        let Ok(sender) = signed_tx.recover(domain) else {
            return None;
        };

        Some(Transaction {
            sender,
            app: signed_tx.message.app,
            nonce: self.nonce,
            max_gas_price: signed_tx.message.max_gas_price,
            data: self.data.clone(),
        })
    }
}

impl proto_message::Signature {
    pub fn from_signature(value: &Signature) -> Self {
        Self {
            v: Some(proto_message::Parity::from_parity(value.v())),
            r: value.r().to_le_bytes_vec(),
            s: value.s().to_le_bytes_vec(),
        }
    }

    pub fn to_signature(&self) -> Signature {
        Signature::new(
            u256_from_bytes(&self.r), 
            u256_from_bytes(&self.s), 
            self.v.unwrap().to_parity()
        )
    }
}

impl proto_message::Parity {
    pub fn from_parity(value: Parity) -> Self {
        match value {
            Parity::Eip155(v) => Self{
                r#type: proto_message::parity::Type::Eip155.into(),
                eip155_value: v,
                non_eip155_value: false,
                parity_value: false,
            },
            Parity::NonEip155(v) => Self{
                r#type: proto_message::parity::Type::NonEip155.into(),
                eip155_value: 0,
                non_eip155_value: v,
                parity_value: false,
            },
            Parity::Parity(v) => Self {
                r#type: proto_message::parity::Type::Parity.into(),
                eip155_value: 0,
                non_eip155_value: false,
                parity_value: v
            },
        }
    }

    pub fn to_parity(&self) -> Parity {
        let parity_type = unsafe { 
            std::mem::transmute::<_, proto_message::parity::Type>(self.r#type)
        };
        match parity_type {
            proto_message::parity::Type::Eip155 => Parity::Eip155(self.eip155_value),
            proto_message::parity::Type::NonEip155 => Parity::NonEip155(self.non_eip155_value),
            proto_message::parity::Type::Parity => Parity::Parity(self.parity_value),
        }
    }
}

pub fn address_to_bytes(address: Address) -> Vec<u8> {
    address.to_vec()
}

pub fn address_from_bytes(bytes: &Vec<u8>) -> Address {
    let b: [u8; 20] = bytes.clone().try_into().unwrap();
    Address::from(b)
}

pub fn u256_to_bytes(u256: U256) -> Vec<u8> {
    u256.to_le_bytes_vec()
}

pub fn u256_from_bytes(bytes: &Vec<u8>) -> U256 {
    let b: [u8; 32] = bytes[..32].try_into().unwrap();
    U256::from_le_bytes(b)
}

pub fn u128_from_bytes(bytes: &Vec<u8>) -> u128 {
    let b: [u8; 16] = bytes[..16].try_into().unwrap();
    u128::from_le_bytes(b)
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct BatchBuilder {
    pub sequencer_payment_address: Address,
    pub txs: Vec<SignedTransaction>,
}

impl BatchBuilder {
    pub fn new(sequencer_payment_address: Address) -> Self {
        Self {
            sequencer_payment_address,
            txs: Vec::new(),
        }
    }

    pub fn add(&mut self, tx: SignedTransaction) {
        self.txs.push(tx)
    }

    pub fn build(self) -> proto_message::Batch {
        let txs = self
            .txs
            .iter()
            .map(proto_message::Transaction::from_signed_transaction)
            .collect();

        proto_message::Batch {
            sequencer_payment_address: self.sequencer_payment_address.to_vec(),
            transactions: txs,
        }
    }
}

#[derive(
    Serialize,
    Deserialize,
    Ord,
    Display,
    PartialOrd,
    PartialEq,
    Eq,
    Hash,
    Debug,
    CanonicalDeserialize,
    CanonicalSerialize,
    Default,
    Clone,
    Copy,
    Into,
)]
#[display(fmt = "{_0}")]
pub struct NamespaceId(u64);

impl From<u64> for NamespaceId {
    fn from(number: u64) -> Self {
        Self(number)
    }
}
#[derive(Serialize, Deserialize, Debug)]
pub struct EspressoTransaction {
    namespace: NamespaceId,
    #[serde(with = "base64_bytes")]
    payload: Vec<u8>,
}

impl EspressoTransaction {
    pub fn new(namespace: NamespaceId, payload: Vec<u8>) -> Self {
        Self { namespace, payload }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SignedTransaction {
    pub message: SigningMessage,
    pub signature: Signature,
}

impl SignedTransaction {
    pub fn valdiate(&self, domain: &Eip712Domain) -> bool {
        self.recover(domain).is_ok()
    }

    pub fn recover(&self, domain: &Eip712Domain) -> Result<Address, SignatureError> {
        let signing_hash = self.message.eip712_signing_hash(domain);
        self.signature.recover_address_from_prehash(&signing_hash)
    }
}

pub const DOMAIN: Eip712Domain = eip712_domain!(
   name: "Cartesi",
   version: "0.1.0",
   chain_id: 11155111,
   verifying_contract: Address::ZERO,
);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct SubmitPointTransaction {
    pub message: String,
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use alloy_core::sol_types::SolStruct;
    use alloy_signer::SignerSync;
    // use alloy_signer_wallet::LocalWallet;
    use alloy_signer_local::PrivateKeySigner as LocalWallet;
    use std::str::FromStr;

    use super::*;

    fn produce_tx() -> (String, Address) {
        let json = r#"
        {
            "app":"0x0000000000000000000000000000000000000000",
            "nonce":0,
            "max_gas_price":0,
            "data":"0x48656c6c6f2c20576f726c6421"
        }
        "#;

        let v: SigningMessage = serde_json::from_str(json).unwrap();
        let signer = LocalWallet::from_str(
            "8114fae7aa0a92c7e3a6015413a54539b4ba9f28254a70f67a3969d73c33509b",
        )
        .unwrap();
        assert_eq!(
            alloy_core::hex::encode(signer.to_field_bytes()),
            "8114fae7aa0a92c7e3a6015413a54539b4ba9f28254a70f67a3969d73c33509b"
        );
        assert_eq!(
            "0x7306897365c277A6951FDA9519fD0CCc16341E4A",
            signer.address().to_string()
        );

        let signature = signer.sign_typed_data_sync(&v, &DOMAIN).unwrap();
        assert_eq!(
            r#"{"r":"0xb131cda9f34ca69a351d2a3b8809a9f0b5f4c99e3e0977d541456d800273cf9e","s":"0x61bb5be8e5a98611fee68ec707a0b4bb901ffc6ffd00c8250d9a7c037fc15680","yParity":"0x1"}"#,
            serde_json::to_string(&signature).unwrap()
        );
        let signed_tx = SignedTransaction {
            message: v,
            signature,
        };

        let ret = serde_json::to_string(&signed_tx).unwrap();

        assert_eq!(
            r#"{"message":{"app":"0x0000000000000000000000000000000000000000","nonce":0,"max_gas_price":0,"data":"0x48656c6c6f2c20576f726c6421"},"signature":{"r":"0xb131cda9f34ca69a351d2a3b8809a9f0b5f4c99e3e0977d541456d800273cf9e","s":"0x61bb5be8e5a98611fee68ec707a0b4bb901ffc6ffd00c8250d9a7c037fc15680","yParity":"0x1"}}"#,
            ret
        );

        (ret, signer.address())
    }

    #[test]
    fn test() {
        let (tx_json, signer) = produce_tx(); // metamask
        println!("JSON: {tx_json}");

        let tx: SignedTransaction = serde_json::from_str(&tx_json).unwrap();
        let signing_hash = tx.message.eip712_signing_hash(&DOMAIN);
        let recovered = tx
            .signature
            .recover_address_from_prehash(&signing_hash)
            .unwrap();

        assert_eq!(signer, recovered);

        assert_eq!(
            r#"{"name":"Cartesi","version":"0.1.0","chainId":"0xaa36a7","verifyingContract":"0x0000000000000000000000000000000000000000"}"#,
            serde_json::to_string(&DOMAIN).unwrap()
        );
    }
}
