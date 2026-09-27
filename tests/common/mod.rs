use plant_journal::{database as db, App};
use sha2::{Digest, Sha256};
pub const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const COOKIE: &str =
    "plant_session=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
// Seed a test-only session in a temporary or explicitly isolated test database.
// Production has no auth bypass; HTTP requests still traverse real middleware.
pub async fn session(app: &App) {
    db::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(format!("{:x}", Sha256::digest(TOKEN.as_bytes())))
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO sessions(token_hash,user_id,garden_id,expires_at) VALUES(?,?,?,?)")
        .bind(format!("{:x}", Sha256::digest(TOKEN.as_bytes())))
        .bind("00000000-0000-0000-0000-000000000002")
        .bind(plant_journal::auth::LEGACY_GARDEN)
        .bind(chrono::Utc::now().timestamp() + 3600)
        .execute(&app.pool)
        .await
        .unwrap();
}
