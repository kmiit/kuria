#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub async fn save_composed_attachments(
    pool: &sqlx::SqlitePool,
    email_id: i64,
    attachments: &[ComposedAttachment],
) -> anyhow::Result<()> {
    for attachment in attachments {
        crate::db::queries::save_attachment(
            pool,
            email_id,
            Some(&attachment.filename),
            Some(&attachment.content_type),
            &attachment.data,
        )
        .await?;
    }

    Ok(())
}
