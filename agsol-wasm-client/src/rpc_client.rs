use super::account::Account;
use super::rpc_config::*;
use super::rpc_request::RpcRequest;
use super::rpc_response::*;

use anyhow::bail;
use borsh::BorshDeserialize;
use log::debug;
use reqwest::header::CONTENT_TYPE;
use serde::de::DeserializeOwned;

use serde_json::json;
use solana_program::pubkey::Pubkey;
use solana_sdk::clock::{Slot, UnixTimestamp};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::{signature::Signature, transaction::Transaction};

use std::str::FromStr;
use std::thread::sleep;
use std::time::Duration;

/// Specifies which Solana cluster will be queried by the client.
#[derive(Clone, Copy, Debug)]
pub enum Net {
    Localhost,
    Testnet,
    Devnet,
    Mainnet,
}

impl Net {
    pub fn to_url(&self) -> &str {
        match self {
            Self::Localhost => "http://localhost:8899",
            Self::Testnet => "https://api.testnet.solana.com",
            Self::Devnet => "https://api.devnet.solana.com",
            Self::Mainnet => "https://api.mainnet-beta.solana.com",
        }
    }
}

pub type ClientResult<T> = Result<T, anyhow::Error>;

/// An async client to make rpc requests to the Solana blockchain.
pub struct RpcClient {
    client: reqwest::Client,
    config: RpcConfig,
    net: Net,
    request_id: u64,
}

impl RpcClient {
    pub fn new_with_config(net: Net, config: RpcConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
            net,
            request_id: 0,
        }
    }

    pub fn new(net: Net) -> Self {
        let config = RpcConfig {
            encoding: Some(Encoding::JsonParsed),
            commitment: Some(CommitmentLevel::Confirmed),
        };
        Self::new_with_config(net, config)
    }

    pub fn set_commitment(&mut self, commitment: Option<CommitmentLevel>) {
        self.config.commitment = commitment;
    }

    async fn send<T: DeserializeOwned, R: Into<reqwest::Body>>(
        &mut self,
        request: R,
    ) -> reqwest::Result<T> {
        self.request_id = self.request_id.wrapping_add(1);
        let response = self
            .client
            .post(self.net.to_url())
            .header(CONTENT_TYPE, "application/json")
            .body(request)
            .send()
            .await?;

        response.json::<T>().await
    }

    /// Returns the decoded contents of a Solana account.
    pub async fn get_account(&mut self, account_pubkey: &Pubkey) -> ClientResult<Account> {
        let request = RpcRequest::GetAccountInfo
            .build_request_json(
                self.request_id,
                json!([account_pubkey.to_string(), self.config]),
            )
            .to_string();
        let response: RpcResponse<RpcResultWithContext<Account>> = self.send(request).await?;
        Ok(response.result.value)
        //let response: serde_json::Value = self.send(request).await?;
        //println!("{:#?}", response);
        //todo!();
    }

    /// Returns the decoded contents of multiple Solana accounts.
    pub async fn get_multiple_accounts(
        &mut self,
        pubkeys: &[Pubkey],
    ) -> ClientResult<Vec<Account>> {
        let pubkeys: Vec<_> = pubkeys.iter().map(|pubkey| pubkey.to_string()).collect();
        let request = RpcRequest::GetMultipleAccounts
            .build_request_json(self.request_id, json!([pubkeys, self.config]))
            .to_string();
        let response: RpcResponse<RpcResultWithContext<Vec<Account>>> = self.send(request).await?;
        Ok(response.result.value)
    }

    /// Attempts to deserialize the contents of an account's data field into a
    /// given type using the Borsh deserialization framework.
    pub async fn get_and_deserialize_account_data<T: BorshDeserialize>(
        &mut self,
        account_pubkey: &Pubkey,
    ) -> ClientResult<T> {
        let account = self.get_account(account_pubkey).await?;
        account.data.parse_into_borsh::<T>()
    }

    /// Attempts to deserialize the contents of an account's data field into a
    /// given type using the Json deserialization framework.
    pub async fn get_and_deserialize_parsed_account_data<T: DeserializeOwned>(
        &mut self,
        account_pubkey: &Pubkey,
    ) -> ClientResult<T> {
        let account = self.get_account(account_pubkey).await?;
        account.data.parse_into_json::<T>()
    }

    /// Returns the owner of the account.
    pub async fn get_owner(&mut self, account_pubkey: &Pubkey) -> ClientResult<Pubkey> {
        let account = self.get_account(account_pubkey).await?;
        let pubkey_bytes = bs58::decode(account.owner).into_vec()?;
        Ok(Pubkey::new(&pubkey_bytes))
    }

    /// Returns the balance (in lamports) of the account.
    pub async fn get_balance(&mut self, account_pubkey: &Pubkey) -> ClientResult<u64> {
        let request = RpcRequest::GetBalance
            .build_request_json(
                self.request_id,
                json!([account_pubkey.to_string(), self.config,]),
            )
            .to_string();

        let response: RpcResponse<RpcResultWithContext<u64>> = self.send(request).await?;
        Ok(response.result.value)
    }

    /// Returns the minimum balance (in Lamports) required for an account to be rent exempt.
    pub async fn get_minimum_balance_for_rent_exemption(
        &mut self,
        data_len: usize,
    ) -> ClientResult<u64> {
        let request = RpcRequest::GetMinimumBalanceForRentExemption
            .build_request_json(self.request_id, json!([data_len]))
            .to_string();

        let response: RpcResponse<u64> = self.send(request).await?;
        Ok(response.result)
    }

    /// Requests an airdrop of lamports to a given account.
    pub async fn request_airdrop(
        &mut self,
        pubkey: &Pubkey,
        lamports: u64,
        recent_blockhash: &Hash,
    ) -> ClientResult<Signature> {
        let config = RpcRequestAirdropConfig {
            recent_blockhash: Some(recent_blockhash.to_string()),
            commitment: self.config.commitment.clone(),
        };
        let request = RpcRequest::RequestAirdrop
            .build_request_json(
                self.request_id,
                json!([pubkey.to_string(), lamports, config]),
            )
            .to_string();

        let response: RpcResponse<String> = self.send(request).await?;

        let signature = Signature::from_str(&response.result)?;
        Ok(signature)
    }

    /// Returns latest blockhash.
    pub async fn get_latest_blockhash(&mut self) -> ClientResult<Hash> {
        // TODO for some reason latest blockhash returns method not found
        // even though we are using 1.9.0 and the rpc servers are also updated
        let request = RpcRequest::GetRecentBlockhash
            .build_request_json(self.request_id, json!([self.config]))
            .to_string();

        let response: RpcResponse<RpcResultWithContext<Blockhash>> = self.send(request).await?;
        let blockhash = Hash::from_str(&response.result.value.blockhash)?;
        Ok(blockhash)
    }

    /// Submit a transaction and wait for confirmation.
    ///
    /// Once this function returns successfully, the given transaction is
    /// guaranteed to be processed with the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// After sending the transaction, this method polls in a loop for the
    /// status of the transaction until it has ben confirmed.
    ///
    pub async fn send_and_confirm_transaction(
        &mut self,
        transaction: &Transaction,
    ) -> ClientResult<Signature> {
        let signature = self.send_transaction(transaction).await?;

        loop {
            let status = self.get_signature_status(&signature).await?;
            if status {
                break;
            }
            sleep(Duration::from_millis(500));
        }

        Ok(signature)
    }

    /// Check if a transaction has been processed with the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// If the transaction has been processed with the given commitment level,
    /// then this method returns `Ok` of `Some`. If the transaction has not yet
    /// been processed with the given commitment level, it returns `Ok` of
    /// `None`.
    pub async fn get_signature_status(&mut self, signature: &Signature) -> ClientResult<bool> {
        let request = RpcRequest::GetSignatureStatuses
            .build_request_json(self.request_id, json!([[signature.to_string()]]))
            .to_string();

        let response: RpcResponse<RpcResultWithContext<Vec<Option<TransactionStatus>>>> =
            self.send(request).await?;

        let commitment: solana_sdk::commitment_config::CommitmentLevel =
            match self.config.commitment {
                Some(CommitmentLevel::Processed) => {
                    solana_sdk::commitment_config::CommitmentLevel::Processed
                }
                Some(CommitmentLevel::Finalized) => {
                    solana_sdk::commitment_config::CommitmentLevel::Finalized
                }
                _ => solana_sdk::commitment_config::CommitmentLevel::Confirmed,
            };

        Ok(response.result.value[0]
            .as_ref()
            .filter(|result| result.satisfies_commitment(CommitmentConfig { commitment }))
            .map(|result| result.status.is_ok())
            .unwrap_or_default())
    }

    /// Attempts to send a signed transaction to the ledger without simulating
    /// it first.
    ///
    /// It is a bit faster, but no logs or confirmation is returned because the
    /// transaction is not simulated.
    pub async fn send_transaction_unchecked(
        &mut self,
        transaction: &Transaction,
    ) -> ClientResult<Signature> {
        let config = RpcTransactionConfig {
            skip_preflight: true,
            preflight_commitment: Some(CommitmentLevel::Processed),
            encoding: Some(Encoding::Base64),
        };
        self.send_transaction_with_config(transaction, &config)
            .await
    }

    pub async fn send_transaction(&mut self, transaction: &Transaction) -> ClientResult<Signature> {
        let config = RpcTransactionConfig {
            skip_preflight: false,
            preflight_commitment: self.config.commitment.clone(),
            encoding: Some(Encoding::Base64),
        };
        self.send_transaction_with_config(transaction, &config)
            .await
    }

    pub async fn send_transaction_with_config(
        &mut self,
        transaction: &Transaction,
        config: &RpcTransactionConfig,
    ) -> ClientResult<Signature> {
        let serialized = bincode::serialize(transaction)?;
        let encoded = base64::encode(serialized);
        let request = RpcRequest::SendTransaction
            .build_request_json(self.request_id, json!([encoded, config]))
            .to_string();

        match self.send::<serde_json::Value, String>(request).await {
            Ok(json_value) => {
                if let Ok(response) =
                    serde_json::from_value::<RpcResponse<String>>(json_value.clone())
                {
                    let signature = Signature::from_str(&response.result)?;
                    Ok(signature)
                } else if let Ok(tx_error) =
                    serde_json::from_value::<RpcResponse<RpcTransactionError>>(json_value)
                {
                    tx_error
                        .result
                        .data
                        .logs
                        .iter()
                        .enumerate()
                        .for_each(|(i, log)| debug!("{} {}", i, log));
                    bail!("{}", tx_error.result.message);
                } else {
                    bail!("failed to parse RPC response")
                }
            }
            Err(err) => bail!(err),
        }
    }

    pub async fn get_slot(&mut self) -> ClientResult<Slot> {
        let request = RpcRequest::GetSlot
            .build_request_json(self.request_id, json!([self.config]))
            .to_string();

        let response: RpcResponse<Slot> = self.send(request).await?;
        Ok(response.result)
    }

    pub async fn get_block_time(&mut self, slot: Slot) -> ClientResult<UnixTimestamp> {
        let request = RpcRequest::GetBlockTime
            .build_request_json(self.request_id, json!([slot]))
            .to_string();

        let response: RpcResponse<UnixTimestamp> = self.send(request).await?;
        Ok(response.result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::account::{ProgramAccount, TokenAccount};
    use solana_sdk::signer::keypair::Keypair;
    use solana_sdk::signer::Signer;
    use solana_sdk::system_transaction::transfer;

    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[rustfmt::skip]
    const ALICE: &[u8] = &[
        57,99,241,156,126,127,97,60,
        40,14,39,4,115,72,39,75,
        2,14,30,255,45,79,195,202,
        132,18,131,180,61,12,87,183,
        14,175,192,115,62,33,136,190,
        244,254,192,174,2,126,227,113,
        222,42,224,89,36,89,239,167,
        22,150,31,29,89,188,176,162
    ];

    #[rustfmt::skip]
    const BOB: &[u8] = &[
        176,252,96,172,240,61,215,84,
        138,250,147,178,208,59,227,60,
        190,204,80,88,55,137,236,252,
        231,118,253,64,65,106,39,5,
        14,212,250,187,124,127,43,205,
        30,117,63,227,13,218,202,68,
        160,161,52,12,59,211,152,183,
        119,140,213,205,174,210,108,128
    ];

    const AIRDROP_AMOUNT: u64 = 5500; // tx free of 5000 lamports included
    const TRANSFER_AMOUNT: u64 = 250;

    async fn wait_for_balance_change(
        client: &mut RpcClient,
        account: &Pubkey,
        balance_before: u64,
        expected_change: u64,
    ) {
        let mut i = 0;
        let max_loops = 60;
        loop {
            let balance_after = client.get_balance(account).await.unwrap();
            // NOTE might happen that alice is airdropped only after she
            // transferred the amount to BOB
            match balance_after.checked_sub(balance_before) {
                Some(0) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    i += 1;
                    dbg!(i);
                }
                Some(delta) => {
                    assert_eq!(delta, expected_change);
                    break;
                }
                None => {
                    assert_eq!(balance_before - balance_after, expected_change);
                    break;
                }
            }
            if i == max_loops {
                panic!("test was running for {} seconds", max_loops);
            }
        }
    }

    #[tokio::test]
    async fn airdrop_and_transfer() {
        let alice = Keypair::from_bytes(ALICE).unwrap();
        let bob = Keypair::from_bytes(BOB).unwrap();
        let mut client = RpcClient::new(Net::Devnet);

        let balance_before_airdrop_alice = client.get_balance(&alice.pubkey()).await.unwrap();
        let latest_blockhash = client.get_latest_blockhash().await.unwrap();

        client
            .request_airdrop(&alice.pubkey(), AIRDROP_AMOUNT, &latest_blockhash)
            .await
            .unwrap();

        wait_for_balance_change(
            &mut client,
            &alice.pubkey(),
            balance_before_airdrop_alice,
            AIRDROP_AMOUNT,
        )
        .await;

        let balance_before_bob = client.get_balance(&bob.pubkey()).await.unwrap();

        let recent_blockhash = client.get_latest_blockhash().await.unwrap();
        let transfer_tx = transfer(&alice, &bob.pubkey(), TRANSFER_AMOUNT, recent_blockhash);
        client.send_transaction(&transfer_tx).await.unwrap(); // Or send_and_confirm_transaction

        wait_for_balance_change(
            &mut client,
            &bob.pubkey(),
            balance_before_bob,
            TRANSFER_AMOUNT,
        )
        .await;

        wait_for_balance_change(
            &mut client,
            &alice.pubkey(),
            balance_before_airdrop_alice,
            TRANSFER_AMOUNT, // also losing the 5000 lamport fee
        )
        .await;
    }

    #[tokio::test]
    async fn block_time() {
        // TODO compare results with solana_client's, once they use
        // spl_token 3.3.0
        let mut client = RpcClient::new(Net::Mainnet);
        for _ in 0..10 {
            let slot = client.get_slot().await.unwrap();
            let block_time = client.get_block_time(slot).await.unwrap();
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            let delta_time = (time - block_time) as f32;
            assert!(delta_time.abs() < 60.0); // we are within one minute
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn get_spl_token_program() {
        let mut client = RpcClient::new(Net::Mainnet);
        client.set_commitment(Some(CommitmentLevel::Processed));
        let pubkey_bytes = bs58::decode("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
            .into_vec()
            .unwrap();
        let token_program_id = Pubkey::new(&pubkey_bytes);

        let account = client.get_account(&token_program_id).await.unwrap();
        assert_eq!(account.owner, "BPFLoader2111111111111111111111111111111111");
        assert!(account.executable);
    }

    #[test]
    fn commitment_change() {
        let config = RpcConfig {
            encoding: Some(Encoding::JsonParsed),
            commitment: None,
        };
        let mut client = RpcClient::new_with_config(Net::Mainnet, config);
        assert!(client.config.commitment.is_none());
        client.set_commitment(Some(CommitmentLevel::Processed));
        assert_eq!(client.config.commitment, Some(CommitmentLevel::Processed));
    }

    #[tokio::test]
    async fn mint_and_token_account() {
        let mut client = RpcClient::new(Net::Mainnet);
        // get NFT mint account from gold.xyz "teletubbies" auction
        let mint_pubkey = Pubkey::new(
            &bs58::decode("B2Kdr5MCJLxJZU1Ek91c6cAkxe1FgFTwEXG6y7cQ9gU7")
                .into_vec()
                .unwrap(),
        );
        let mint = client
            .get_and_deserialize_parsed_account_data::<TokenAccount>(&mint_pubkey)
            .await
            .unwrap();
        let mint_info = if let TokenAccount::Mint(mint_info) = mint {
            mint_info
        } else {
            panic!("should be mint account");
        };
        assert_eq!(mint_info.decimals, 0);
        assert_eq!(mint_info.supply.parse::<u8>().unwrap(), 1);
        // get NFT token account from gold.xyz "teletubbies" auction
        let token_account_pubkey = Pubkey::new(
            &bs58::decode("6xrSzvKGBux6FHZdRuKwrWwHxCcwdgfTVFVUaiPbsmSR")
                .into_vec()
                .unwrap(),
        );
        let token_account = client
            .get_and_deserialize_parsed_account_data::<TokenAccount>(&token_account_pubkey)
            .await
            .unwrap();

        let token_acc_info = if let TokenAccount::Account(account_info) = token_account {
            account_info
        } else {
            panic!("should be token account");
        };
        assert_eq!(token_acc_info.mint, mint_pubkey.to_string())
    }

    #[tokio::test]
    async fn deserialize_go1d_account() {
        let mut client = RpcClient::new(Net::Mainnet);
        let gold_pubkey = Pubkey::new(
            &bs58::decode("go1dcKcvafq8SDwmBKo6t2NVzyhvTEZJkMwnnfae99U")
                .into_vec()
                .unwrap(),
        );

        let gold_acc = client
            .get_and_deserialize_parsed_account_data::<ProgramAccount>(&gold_pubkey)
            .await
            .unwrap();

        if let ProgramAccount::Program(_program) = gold_acc {
        } else {
            panic!("should be a program account");
        }
    }

    #[derive(BorshDeserialize)]
    struct GoldContractBankState {
        admin: Pubkey,
        wd_auth: Pubkey,
    }

    #[tokio::test]
    async fn get_borsh_serialized_account_data() {
        let mut client = RpcClient::new(Net::Mainnet);
        let contract_pubkey = Pubkey::new(
            &bs58::decode("21d8ssndpeW5mw1EMqVZRNHnJhUfuWkKL7QomWF87LBK")
                .into_vec()
                .unwrap(),
        );
        let contract_state = client
            .get_and_deserialize_account_data::<GoldContractBankState>(&contract_pubkey)
            .await
            .unwrap();

        assert_eq!(
            contract_state.admin.to_string(),
            "gcadHFMc51A2fFzppTQ6DgmLNymatHjGwENZSkJpJNr"
        );
        assert_ne!(contract_state.admin, contract_state.wd_auth);
    }

    #[tokio::test]
    async fn get_multiple_accounts() {
        let mut client = RpcClient::new(Net::Mainnet);
        client.set_commitment(Some(CommitmentLevel::Processed));

        let token_program_id =
            Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();

        let contract_pubkey =
            Pubkey::from_str("21d8ssndpeW5mw1EMqVZRNHnJhUfuWkKL7QomWF87LBK").unwrap();

        let accounts = client
            .get_multiple_accounts(&[token_program_id, contract_pubkey])
            .await
            .unwrap();
        assert_eq!(
            accounts[0].owner,
            "BPFLoader2111111111111111111111111111111111"
        );
        assert!(accounts[0].executable);
        assert_eq!(
            accounts[1].owner,
            "go1dcKcvafq8SDwmBKo6t2NVzyhvTEZJkMwnnfae99U"
        );
        assert!(!accounts[1].executable);
    }
}
