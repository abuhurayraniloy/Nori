use nori_cli::commands::{run_speed, SpeedArgs};
use nori_cli::protocol::SearchResponse;
use nori_cli::service::{SearchService, SystemLoad, Watchdog};
use tempfile::tempdir;

#[test]
fn test_system_load_sample() {
    let sample = SystemLoad::sample();
    // On Windows, sample should succeed
    #[cfg(windows)]
    assert!(sample.is_some());
}

#[test]
fn test_watchdog_footprint() {
    let footprint = Watchdog::current_memory_footprint();
    #[cfg(windows)]
    {
        assert!(footprint.is_some());
        assert!(footprint.unwrap() > 0);
    }
}

#[test]
fn test_speed_command() {
    let args = SpeedArgs { tries: 1, json: false };
    assert!(run_speed(args).is_ok());
}

#[test]
fn test_search_service_protocol() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.sqlite");

    let mut service = SearchService::new(&db_path, false).expect("failed to init SearchService");

    // 1. Status request
    let line = r#"{"id": 1, "op": "status"}"#;
    let resp_str = service.handle_line(line).expect("expected response");
    let resp: SearchResponse = serde_json::from_str(&resp_str).unwrap();
    assert_eq!(resp.ok, Some(true));
    assert!(resp.status.is_some());

    // 2. Quit request
    let quit_line = r#"{"id": 2, "op": "quit"}"#;
    let quit_resp_str = service.handle_line(quit_line).expect("expected response");
    let quit_resp: SearchResponse = serde_json::from_str(&quit_resp_str).unwrap();
    assert_eq!(quit_resp.ok, Some(true));
    assert!(service.is_quitting());
}
