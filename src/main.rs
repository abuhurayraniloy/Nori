mod db;

fn main() {
    println!("🚀 Starting Nori initialization...");

    match db::init_db() {
        Ok(_conn) => {
            println!("✅ Connected to SQLite Database: nori.db");
        }
        Err(e) => {
            eprintln!("❌ Failed to connect to SQLite Database: {}", e);
        }
    }
}
