use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use crate::config::Config;

pub async fn handle_connection(
    stream: TcpStream,
    _config: Arc<Config>,
    db: sqlx::SqlitePool,
    _is_tls: bool,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);

    send_line(&mut reader, "+OK Kuria POP3 server ready\r\n").await?;

    let mut authenticated = false;
    let mut username = String::new();
    let mut user_id = 0i64;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts[0].to_uppercase();

        match cmd.as_str() {
            "USER" => {
                if parts.len() < 2 {
                    send_line(&mut reader, "-ERR Missing username\r\n").await?;
                    continue;
                }
                username = parts[1].to_string();
                send_line(&mut reader, "+OK User accepted\r\n").await?;
            }
            "PASS" => {
                if parts.len() < 2 {
                    send_line(&mut reader, "-ERR Missing password\r\n").await?;
                    continue;
                }
                let password = parts[1];

                match crate::db::queries::get_user_by_email(&db, &username).await {
                    Ok(Some(user)) => {
                        if bcrypt::verify(password, &user.password_hash).unwrap_or(false) {
                            authenticated = true;
                            user_id = user.id;
                            send_line(&mut reader, "+OK Logged in\r\n").await?;
                        } else {
                            send_line(&mut reader, "-ERR Authentication failed\r\n").await?;
                        }
                    }
                    _ => {
                        send_line(&mut reader, "-ERR Authentication failed\r\n").await?;
                    }
                }
            }
            "STAT" => {
                if !authenticated {
                    send_line(&mut reader, "-ERR Not authenticated\r\n").await?;
                    continue;
                }
                let count = count_messages(&db, user_id).await?;
                let size = total_size(&db, user_id).await?;
                send_line(&mut reader, &format!("+OK {} {}\r\n", count, size)).await?;
            }
            "LIST" => {
                if !authenticated {
                    send_line(&mut reader, "-ERR Not authenticated\r\n").await?;
                    continue;
                }
                let messages = list_messages(&db, user_id).await?;
                send_line(&mut reader, &format!("+OK {} messages\r\n", messages.len())).await?;
                for (i, size) in messages.iter().enumerate() {
                    send_line(&mut reader, &format!("{} {}\r\n", i + 1, size)).await?;
                }
                send_line(&mut reader, ".\r\n").await?;
            }
            "RETR" => {
                if !authenticated {
                    send_line(&mut reader, "-ERR Not authenticated\r\n").await?;
                    continue;
                }
                if parts.len() < 2 {
                    send_line(&mut reader, "-ERR Missing message number\r\n").await?;
                    continue;
                }
                let msg_num: usize = parts[1].parse().unwrap_or(0);
                if msg_num == 0 {
                    send_line(&mut reader, "-ERR Invalid message number\r\n").await?;
                    continue;
                }

                match retrieve_message(&db, user_id, msg_num).await {
                    Ok(content) => {
                        send_line(&mut reader, &format!("+OK {} octets\r\n", content.len())).await?;
                        send_line(&mut reader, &content).await?;
                        send_line(&mut reader, "\r\n.\r\n").await?;
                    }
                    Err(_) => {
                        send_line(&mut reader, "-ERR No such message\r\n").await?;
                    }
                }
            }
            "DELE" => {
                if !authenticated {
                    send_line(&mut reader, "-ERR Not authenticated\r\n").await?;
                    continue;
                }
                if parts.len() < 2 {
                    send_line(&mut reader, "-ERR Missing message number\r\n").await?;
                    continue;
                }
                let msg_num: usize = parts[1].parse().unwrap_or(0);
                if msg_num == 0 {
                    send_line(&mut reader, "-ERR Invalid message number\r\n").await?;
                    continue;
                }

                if delete_message(&db, user_id, msg_num).await.is_ok() {
                    send_line(&mut reader, "+OK Message deleted\r\n").await?;
                } else {
                    send_line(&mut reader, "-ERR No such message\r\n").await?;
                }
            }
            "QUIT" => {
                send_line(&mut reader, "+OK Goodbye\r\n").await?;
                break;
            }
            "NOOP" => {
                send_line(&mut reader, "+OK\r\n").await?;
            }
            "CAPA" => {
                send_line(&mut reader, "+OK Capability list follows\r\n").await?;
                send_line(&mut reader, "USER\r\n").await?;
                send_line(&mut reader, ".\r\n").await?;
            }
            _ => {
                send_line(&mut reader, "-ERR Unknown command\r\n").await?;
            }
        }
    }

    Ok(())
}

async fn send_line(reader: &mut BufReader<TcpStream>, data: &str) -> anyhow::Result<()> {
    reader.get_mut().write_all(data.as_bytes()).await?;
    reader.get_mut().flush().await?;
    Ok(())
}

async fn count_messages(db: &sqlx::SqlitePool, user_id: i64) -> anyhow::Result<i64> {
    let result = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM mailbox WHERE user_id = ? AND deleted = 0"
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;
    Ok(result)
}

async fn total_size(db: &sqlx::SqlitePool, user_id: i64) -> anyhow::Result<i64> {
    let result = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT SUM(LENGTH(raw)) FROM mailbox WHERE user_id = ? AND deleted = 0"
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;
    Ok(result.unwrap_or(0))
}

async fn list_messages(db: &sqlx::SqlitePool, user_id: i64) -> anyhow::Result<Vec<i64>> {
    let results = sqlx::query_scalar::<_, i64>(
        "SELECT LENGTH(raw) FROM mailbox WHERE user_id = ? AND deleted = 0 ORDER BY id"
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(results)
}

async fn retrieve_message(db: &sqlx::SqlitePool, user_id: i64, msg_num: usize) -> anyhow::Result<String> {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT raw FROM mailbox WHERE user_id = ? AND deleted = 0 ORDER BY id LIMIT 1 OFFSET ?"
    )
    .bind(user_id)
    .bind((msg_num - 1) as i64)
    .fetch_one(db)
    .await?;
    Ok(result)
}

async fn delete_message(db: &sqlx::SqlitePool, user_id: i64, msg_num: usize) -> anyhow::Result<()> {
    let affected = sqlx::query(
        "UPDATE mailbox SET deleted = 1 WHERE id = (SELECT id FROM mailbox WHERE user_id = ? AND deleted = 0 ORDER BY id LIMIT 1 OFFSET ?)"
    )
    .bind(user_id)
    .bind((msg_num - 1) as i64)
    .execute(db)
    .await?
    .rows_affected();

    if affected == 0 {
        anyhow::bail!("No such message");
    }
    Ok(())
}
