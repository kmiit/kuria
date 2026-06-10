use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

/// Standard success response with data
#[allow(dead_code)]
pub fn success<T: serde::Serialize>(data: T) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(json!(data)))
}

/// Standard success response with simple "ok" flag
pub fn ok() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(json!({ "ok": true })))
}

/// Standard error response with message and status code
pub fn error(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "error": message.into(),
            "status": status.as_u16(),
        })),
    )
}

/// Standard error response with just status code (uses default message)
pub fn error_status(status: StatusCode) -> (StatusCode, Json<serde_json::Value>) {
    error(status, default_error_message(status))
}

/// Get default error message for status code
fn default_error_message(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "Bad request",
        StatusCode::UNAUTHORIZED => "Unauthorized",
        StatusCode::FORBIDDEN => "Forbidden",
        StatusCode::NOT_FOUND => "Not found",
        StatusCode::CONFLICT => "Conflict",
        StatusCode::PAYLOAD_TOO_LARGE => "Payload too large",
        StatusCode::TOO_MANY_REQUESTS => "Too many requests",
        StatusCode::UNPROCESSABLE_ENTITY => "Invalid request body",
        StatusCode::INTERNAL_SERVER_ERROR => "Internal server error",
        _ => status.canonical_reason().unwrap_or("Request failed"),
    }
}

/// Create paginated list response
#[allow(dead_code)]
pub fn paginated<T: serde::Serialize>(
    items: T,
    total: i64,
    page: i64,
    per_page: i64,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "items": items,
            "total": total,
            "page": page,
            "per_page": per_page,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_response_wraps_data() {
        let (status, Json(body)) = success(json!({ "user": "test" }));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["user"], "test");
    }

    #[test]
    fn ok_response_has_ok_flag() {
        let (status, Json(body)) = ok();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
    }

    #[test]
    fn error_response_includes_status_and_message() {
        let (status, Json(body)) = error(StatusCode::BAD_REQUEST, "Invalid input");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Invalid input");
        assert_eq!(body["status"], 400);
    }

    #[test]
    fn error_status_uses_default_messages() {
        let (_, Json(body)) = error_status(StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "Not found");
        assert_eq!(body["status"], 404);
    }

    #[test]
    fn paginated_response_includes_pagination_metadata() {
        let items = vec!["item1", "item2"];
        let (status, Json(body)) = paginated(&items, 100, 2, 50);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["items"], json!(items));
        assert_eq!(body["total"], 100);
        assert_eq!(body["page"], 2);
        assert_eq!(body["per_page"], 50);
    }
}
