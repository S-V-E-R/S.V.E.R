//! The double-entry ledger (docs/SUPPORT.md "Payments and the ledger"). Each account's balance is
//! the sum of its entries; positive means held by that account. The database rejects a
//! transaction whose entries don't sum to zero per unit, and entries can't be changed afterwards.
//!
//! Accounts: `valor:<user>` (Purchased Valor), `valor:issued` (Valor sold), `valor:spent`;
//! `usd:stripe` (cash collected), `usd:sales`, `usd:platform`, `usd:earnings:<user>` (owed to a
//! creator). USD is in tenths of a cent.
use crate::profiles::Res;
use serde_json::Value;
use sqlx::PgConnection;

/// Tenths of a cent a creator earns per Valor spent on them (0.8¢).
pub const EARN_PER_VALOR: i64 = 8;

/// Posts one balanced transaction. Returns false when `reference` was already posted, so a
/// replayed webhook or retried request never moves money twice.
pub async fn post(
    db: &mut PgConnection,
    kind: &str,
    reference: &str,
    detail: Value,
    entries: &[(&str, &str, i64)],
) -> Res<bool> {
    let id = crate::profiles::new_id();
    let inserted = sqlx::query("INSERT INTO ledger_transactions(id,kind,reference,detail) VALUES($1,$2,$3,$4) ON CONFLICT(reference) DO NOTHING")
        .bind(&id).bind(kind).bind(reference).bind(detail)
        .execute(&mut *db).await?.rows_affected();
    if inserted == 0 {
        return Ok(false);
    }
    for (account, unit, amount) in entries.iter().filter(|e| e.2 != 0) {
        sqlx::query(
            "INSERT INTO ledger_entries(transaction_id,account,unit,amount) VALUES($1,$2,$3,$4)",
        )
        .bind(&id)
        .bind(account)
        .bind(unit)
        .bind(amount)
        .execute(&mut *db)
        .await?;
    }
    Ok(true)
}

pub async fn balance(db: &mut PgConnection, account: &str, unit: &str) -> Res<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(sum(amount),0)::bigint FROM ledger_entries WHERE account=$1 AND unit=$2",
    )
    .bind(account)
    .bind(unit)
    .fetch_one(db)
    .await?)
}

/// Serializes spending from one account inside the caller's transaction, so two tributes can't
/// both pass the balance check.
pub async fn lock(db: &mut PgConnection, account: &str) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 6))")
        .bind(account)
        .execute(db)
        .await?;
    Ok(())
}
