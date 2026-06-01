use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    pub ok: bool,
    pub service: String,
}

#[tauri::command]
pub fn ping() -> HealthStatus {
    HealthStatus {
        ok: true,
        service: "yuuko-news-backend".to_string(),
    }
}
