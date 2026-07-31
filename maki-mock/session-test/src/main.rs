use maki_storage::StateDir;
use maki_storage::sessions::Session;
use maki_storage::id::MakiId;
use maki_providers::Message;
use maki_providers::TokenUsage;

fn main() {
    eprintln!("[TEST] StateDir::resolve() 测试");
    match StateDir::resolve() {
        Ok(dir) => {
            eprintln!("[TEST] StateDir::resolve OK: {}", dir.path().display());
            let sessions_dir = dir.ensure_subdir("sessions").unwrap();
            eprintln!("[TEST] sessions dir: {}", sessions_dir.display());

            let id: MakiId = "CdZuiZUNCf1c715Qh1etW".parse().unwrap();
            let mut session = Session::<Message, TokenUsage, serde_json::Value>::new("test/model", "/_ext/home/headless");
            session.id = id;
            match session.save(&dir) {
                Ok(_) => eprintln!("[TEST] session.save OK"),
                Err(e) => eprintln!("[TEST] session.save FAILED: {e}"),
            }
        }
        Err(e) => eprintln!("[TEST] StateDir::resolve FAILED: {e}"),
    }
}
