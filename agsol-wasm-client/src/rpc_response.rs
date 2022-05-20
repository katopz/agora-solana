use serde::Deserialize;
use solana_program::clock::Slot;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    transaction::{Result as TransactionResult, TransactionError},
};

#[derive(Deserialize, Debug)]
pub struct RpcResponse<T> {
    pub id: u64,
    pub jsonrpc: String,
    #[serde(alias = "error")]
    pub result: T,
}

#[derive(Deserialize, Debug)]
pub struct Context {
    pub slot: u64,
}

#[derive(Deserialize, Debug)]
pub struct RpcResultWithContext<T> {
    pub context: Context,
    pub value: T,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Blockhash {
    pub blockhash: String,
    #[serde(skip)] // TODO latest blockhash
    pub last_valid_block_height: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionError {
    pub code: i64,
    pub data: RpcTransactionErrorData,
    pub message: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionErrorData {
    pub err: TransactionError,
    pub logs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionConfirmationStatus {
    Processed,
    Confirmed,
    Finalized,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct TransactionStatus {
    pub slot: Slot,
    pub confirmations: Option<usize>,  // None = rooted
    pub status: TransactionResult<()>, // legacy field
    pub err: Option<TransactionError>,
    pub confirmation_status: Option<TransactionConfirmationStatus>,
}

impl TransactionStatus {
    pub fn satisfies_commitment(&self, commitment_config: CommitmentConfig) -> bool {
        if commitment_config.is_finalized() {
            self.confirmations.is_none()
        } else if commitment_config.is_confirmed() {
            if let Some(status) = &self.confirmation_status {
                *status != TransactionConfirmationStatus::Processed
            } else {
                // These fallback cases handle TransactionStatus RPC responses from older software
                self.confirmations.is_some() && self.confirmations.unwrap() > 1
                    || self.confirmations.is_none()
            }
        } else {
            true
        }
    }

    // Returns `confirmation_status`, or if is_none, determines the status from confirmations.
    // Facilitates querying nodes on older software
    #[allow(dead_code)]
    pub fn confirmation_status(&self) -> TransactionConfirmationStatus {
        match &self.confirmation_status {
            Some(status) => status.clone(),
            None => {
                if self.confirmations.is_none() {
                    TransactionConfirmationStatus::Finalized
                } else if self.confirmations.unwrap() > 0 {
                    TransactionConfirmationStatus::Confirmed
                } else {
                    TransactionConfirmationStatus::Processed
                }
            }
        }
    }
}
