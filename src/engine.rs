use anyhow::{Context, Error};
use rust_decimal::prelude::FromStr;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;

#[derive(Debug, Deserialize)]
pub struct Transaction {
    #[serde(rename(deserialize = "type"))]
    tx_type: TransactionType,
    #[serde(rename(deserialize = "client"))]
    client_id: u16,
    #[serde(rename(deserialize = "tx"))]
    tx_id: u32,
    amount: Option<String>,
}

impl Transaction {
    /// Used to convert the transaction amount to a decimal number so we can perform math on it.
    fn amount(&self) -> anyhow::Result<Decimal> {
        let amount = self.amount.as_ref().context("Amount was empty")?;
        Decimal::from_str(amount).context("Failed to deserialize amount")
    }
}

#[cfg(test)]
impl Transaction {
    // A useful constructor for testing
    fn from(
        tx_type: TransactionType,
        client_id: u16,
        tx_id: u32,
        amount: Option<impl Into<String>>,
    ) -> Self {
        let amount: Option<String> = amount.map(|amt| amt.into());
        Self {
            tx_type,
            client_id,
            tx_id,
            amount,
        }
    }
}

#[derive(Debug, Deserialize)]
enum TransactionType {
    #[serde(rename(deserialize = "deposit"))]
    Deposit,
    #[serde(rename(deserialize = "withdrawal"))]
    Withdrawal,
    #[serde(rename(deserialize = "dispute"))]
    Dispute,
    #[serde(rename(deserialize = "resolve"))]
    Resolve,
    #[serde(rename(deserialize = "chargeback"))]
    Chargeback,
}

#[derive(Default, Debug, Clone, Copy)]
struct Account {
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
}

#[derive(Debug)]
pub struct AccountWithId {
    id: u16,
    account: Account,
}

impl Display for AccountWithId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{:.4},{:.4},{:.4},{}",
            self.id,
            self.account.available,
            self.account.held,
            self.account.total.round_dp(4),
            self.account.locked
        )
    }
}

#[derive(Debug)]
pub struct TransactionEngine {
    // The state of every account indexed by the account Id
    accounts: HashMap<u16, Account>,
    // All transactions that have been seen that are currently eligible to be disputed indexed by
    // the transaction Id
    transactions: HashMap<u32, Transaction>,
    // The set of transaction Ids that are currently in dispute
    disputed_transactions: HashSet<u32>,
}

impl TransactionEngine {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
            disputed_transactions: HashSet::new(),
        }
    }

    /// Processes the given transaction creating & updating the client's account as necessary.
    pub fn process_transaction(&mut self, tx: Transaction) -> anyhow::Result<()> {
        // If this is the first transaction for the client create an account and insert that
        // otherwise get the existing account
        let tx_account = self
            .accounts
            .entry(tx.client_id)
            .or_insert_with(Account::default);

        // If the account is locked we won't do any further processing
        if tx_account.locked {
            // It may be better to treat this as an error case
            return anyhow::Result::Ok(());
        }

        // Take appropriate action based on the transaction type
        match tx.tx_type {
            TransactionType::Deposit => {
                let tx_amount = tx.amount().context("Failed to get deposit amount")?;
                tx_account.total += tx_amount;
                tx_account.available += tx_amount;
                // Store this transaction in case of later dispute
                self.transactions.insert(tx.tx_id, tx);
            }
            TransactionType::Withdrawal => {
                let tx_amount = tx.amount().context("Failed to get withdrawal amount")?;
                // Only process this withdrawal if the account has sufficient available funds
                if tx_account.available >= tx_amount {
                    tx_account.total -= tx_amount;
                    tx_account.available -= tx_amount;
                    // Store this transaction in case of later dispute
                    self.transactions.insert(tx.tx_id, tx);
                }
            }
            TransactionType::Dispute => {
                // Only dispute this transaction if the transaction Id refers to a valid transaction
                if let Some(disputed_tx) = self.transactions.get(&tx.tx_id) {
                    let disputed_tx_amount = disputed_tx
                        .amount()
                        .context("Failed to get disputed transaction amount")?;
                    match disputed_tx.tx_type {
                        TransactionType::Deposit => {
                            tx_account.available -= disputed_tx_amount;
                            tx_account.held += disputed_tx_amount;
                        }
                        TransactionType::Withdrawal => {
                            tx_account.total += disputed_tx_amount;
                            tx_account.held += disputed_tx_amount;
                        }
                        _ => return Err(Error::msg("Invalid disputed transaction")),
                    }
                    self.disputed_transactions.insert(disputed_tx.tx_id);
                }
            }
            TransactionType::Resolve => {
                // The transaction must both refer to a valid existing transaction and that
                // transaction must be currently disputed in order for us to process a resolve
                if let Some(disputed_tx) = self.transactions.get(&tx.tx_id) {
                    if self.disputed_transactions.contains(&tx.tx_id) {
                        let disputed_tx_amount = disputed_tx
                            .amount()
                            .context("Failed to get disputed transaction amount")?;
                        match disputed_tx.tx_type {
                            TransactionType::Deposit => {
                                tx_account.held -= disputed_tx_amount;
                                tx_account.available += disputed_tx_amount;
                            }
                            TransactionType::Withdrawal => {
                                tx_account.total -= disputed_tx_amount;
                                tx_account.held -= disputed_tx_amount;
                            }
                            _ => return Err(Error::msg("Invalid disputed transaction")),
                        }
                        // Now that we have processed the resolve we can mark the transaction as no
                        // longer disputed
                        self.disputed_transactions.remove(&tx.tx_id);
                    }
                }
            }
            TransactionType::Chargeback => {
                // The transaction must both refer to a valid existing transaction and that
                // transaction must be currently disputed in order for us to process a chargeback
                if let Some(disputed_tx) = self.transactions.get(&tx.tx_id) {
                    if self.disputed_transactions.contains(&tx.tx_id) {
                        let disputed_tx_amount = disputed_tx
                            .amount()
                            .context("Failed to get disputed transaction amount")?;
                        match disputed_tx.tx_type {
                            TransactionType::Deposit => {
                                tx_account.held -= disputed_tx_amount;
                                tx_account.total -= disputed_tx_amount;
                            }
                            TransactionType::Withdrawal => {
                                tx_account.held -= disputed_tx_amount;
                                tx_account.available += disputed_tx_amount;
                            }
                            _ => return Err(Error::msg("Invalid disputed transaction")),
                        }
                        // Now that we have processed the chargeback we can mark the
                        // transaction as no longer disputed
                        self.disputed_transactions.remove(&tx.tx_id);
                        // Processing a chargeback results in locking of the client's
                        // account
                        tx_account.locked = true
                    }
                }
            }
        }
        anyhow::Result::Ok(())
    }

    /// Retrieve an iterator of all the accounts including their Ids. This function retrieves the
    /// state of all accounts as of a particular point in time. The account information is given
    /// in the form of immutable copies as at the time the iterator is iterated.
    pub fn retrieve_accounts(&self) -> impl Iterator<Item = AccountWithId> + '_ {
        self.accounts.iter().map(|(id, account)| AccountWithId {
            // Copy out the entries values
            id: *id,
            account: *account,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::TransactionType::Chargeback;
    use crate::engine::TransactionType::Deposit;
    use crate::engine::TransactionType::Dispute;
    use crate::engine::TransactionType::Resolve;
    use crate::engine::TransactionType::Withdrawal;
    use rust_decimal::prelude::FromStr;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn can_deposit_and_withdraw() {
        let mut engine = TransactionEngine::new();
        let acct_id = 1;
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 1, Some("1.0")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        assert_eq!(current_acct.available, dec("1.0"));
        engine
            .process_transaction(Transaction::from(Withdrawal, acct_id, 1, Some("0.1234")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        assert_eq!(current_acct.available, dec("0.8766"));
    }

    #[test]
    fn chargeback_deposit_flow() {
        let mut engine = TransactionEngine::new();
        let acct_id = 1;
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 1, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Dispute, acct_id, 1, Option::<&str>::None))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Available and held should have been modified due to the dispute
        assert_eq!(current_acct.available, dec("0"));
        assert_eq!(current_acct.held, dec("1.0"));
        assert_eq!(engine.disputed_transactions.contains(&1), true);
        engine
            .process_transaction(Transaction::from(
                Chargeback,
                acct_id,
                1,
                Option::<&str>::None,
            ))
            .unwrap();
        // Now that a chargeback has occurred the account should be empty and locked
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        assert_eq!(current_acct.available, dec("0"));
        assert_eq!(current_acct.held, dec("0"));
        assert_eq!(current_acct.locked, true);
        assert_eq!(engine.disputed_transactions.is_empty(), true);
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 2, Some("1.0")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Since we are locked we shouldn't be able to deposit anymore
        assert_eq!(current_acct.total, dec("0"));
    }

    #[test]
    fn resolve_deposit_flow() {
        let mut engine = TransactionEngine::new();
        let acct_id = 1;
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 1, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Dispute, acct_id, 1, Option::<&str>::None))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Available and held should have been modified due to the dispute
        assert_eq!(current_acct.available, dec("0"));
        assert_eq!(current_acct.held, dec("1.0"));
        assert_eq!(engine.disputed_transactions.contains(&1), true);
        engine
            .process_transaction(Transaction::from(Resolve, acct_id, 1, Option::<&str>::None))
            .unwrap();
        // Now that a resolve has occurred the account should have funds restored
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        assert_eq!(current_acct.available, dec("1.0"));
        assert_eq!(current_acct.held, dec("0"));
        assert_eq!(current_acct.locked, false);
        assert_eq!(engine.disputed_transactions.is_empty(), true);
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 2, Some("1.0")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Additional deposits should be fine
        assert_eq!(current_acct.available, dec("2.0"));
    }

    #[test]
    fn resolve_withdrawal_flow() {
        let mut engine = TransactionEngine::new();
        let acct_id = 1;
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 1, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Withdrawal, acct_id, 2, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Dispute, acct_id, 2, Option::<&str>::None))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Available and held should have been modified due to the dispute
        assert_eq!(current_acct.available, dec("0"));
        assert_eq!(current_acct.held, dec("1.0"));
        assert_eq!(current_acct.total, dec("1.0"));
        assert_eq!(engine.disputed_transactions.contains(&2), true);
        engine
            .process_transaction(Transaction::from(Resolve, acct_id, 2, Option::<&str>::None))
            .unwrap();
        // Now that a resolve has occurred the account should have funds restored
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        assert_eq!(current_acct.available, dec("0"));
        assert_eq!(current_acct.held, dec("0"));
        assert_eq!(current_acct.locked, false);
        assert_eq!(engine.disputed_transactions.is_empty(), true);
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 3, Some("1.0")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // Additional deposits should be fine
        assert_eq!(current_acct.available, dec("1.0"));
    }

    #[test]
    fn withdraw_too_much() {
        let mut engine = TransactionEngine::new();
        let acct_id = 1;
        engine
            .process_transaction(Transaction::from(Deposit, acct_id, 1, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Withdrawal, acct_id, 1, Some("2.0")))
            .unwrap();
        let current_acct = engine.accounts.get(&acct_id).unwrap();
        // The withdrawal should not have had an effect
        assert_eq!(current_acct.available, dec("1.0"));
    }

    #[test]
    #[ignore]
    fn basic_sanity() {
        let mut engine = TransactionEngine::new();
        engine
            .process_transaction(Transaction::from(Deposit, 1, 1, Some("1.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Deposit, 2, 2, Some("2.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Deposit, 1, 3, Some("2.0")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Withdrawal, 1, 4, Some("1.5")))
            .unwrap();
        engine
            .process_transaction(Transaction::from(Withdrawal, 2, 5, Some("3.0")))
            .unwrap();
        engine
            .retrieve_accounts()
            .for_each(|acct| eprintln!("{}", acct));
    }
}
