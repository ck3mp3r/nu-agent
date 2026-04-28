#[test]
fn test_logger_initialization() {
    // Logger can only be initialized once per process
    // This test verifies it doesn't panic on init

    // Note: env_logger::try_init() returns error if already initialized
    // which is fine - we just want to ensure it's safe to call
    let _ = env_logger::try_init();

    // Verify logging works
    log::debug!("Test debug message");
    log::info!("Test info message");
}
