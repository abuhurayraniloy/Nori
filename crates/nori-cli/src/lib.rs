//! nori-cli: Developer command-line driver and background helper services for Nori.

pub mod commands;
pub mod protocol;
pub mod service;

pub use commands::{
    run_estimate, run_index, run_search, run_speed, run_status, EstimateArgs, IndexArgs,
    SearchArgs, SpeedArgs, StatusArgs,
};
pub use protocol::{
    IndexEvent, SearchHit, SearchRequest, SearchResponse, SearchTiming,
};
pub use service::{
    IndexService, SearchService, StoppedFile, SystemLoad, SystemLoadSample, Watchdog,
    WorkerService, WATCHDOG_EXIT_CODE,
};
