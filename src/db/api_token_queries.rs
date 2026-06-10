use super::models::ApiToken;
use sqlx::SqlitePool;

// API Token queries
pub async fn create_api_token(
    pool: &SqlitePool,
    user_id: i64,
    token: &str,
    name: &str,
) -> anyhow::Result<ApiToken> {
    let api_token = sqlx::query_as::<_, ApiToken>(
        "INSERT INTO api_tokens (user_id, token, name) VALUES (?, ?, ?) RETURNING *",
    )
    .bind(user_id)
    .bind(token)
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(api_token)
}

pub async fn get_api_token_by_token(
    pool: &SqlitePool,
    token: &str,
) -> anyhow::Result<Option<ApiToken>> {
    let api_token = sqlx::query_as::<_, ApiToken>("SELECT * FROM api_tokens WHERE token = ?")
        .bind(token)
        .fetch_optional(pool)
        .await?;
    Ok(api_token)
}

pub async fn list_api_tokens_by_user(
    pool: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<Vec<ApiToken>> {
    let tokens = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens WHERE user_id = ? ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(tokens)
}

pub async fn delete_api_token(
    pool: &SqlitePool,
    token_id: i64,
    user_id: i64,
) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM api_tokens WHERE id = ? AND user_id = ?")
        .bind(token_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_api_token_last_used(pool: &SqlitePool, token: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE api_tokens SET last_used_at = CURRENT_TIMESTAMP WHERE token = ?")
        .bind(token)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_user_api_access(
    pool: &SqlitePool,
    user_id: i64,
    api_enabled: bool,
) -> anyhow::Result<bool> {
    let result = sqlx::query("UPDATE users SET api_enabled = ? WHERE id = ?")
        .bind(api_enabled)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_all_user_api_tokens(pool: &SqlitePool, user_id: i64) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM api_tokens WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
