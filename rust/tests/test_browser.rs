use scavenger::plugins::BotDetectedError;

#[test]
fn bot_detected_error_formatting() {
    let err = BotDetectedError {
        plugin_id: "ebay".into(),
        url: "https://ebay.com/search".into(),
        message: "CAPTCHA presented".into(),
    };
    let msg = err.to_string();
    assert_eq!(msg, "Bot detected on ebay: CAPTCHA presented");
    assert_eq!(err.url, "https://ebay.com/search");
}

#[test]
fn bot_detected_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BotDetectedError>();
}

#[test]
fn plugin_trait_is_object_safe() {
    // This compiles only if Plugin is object-safe.
    fn _accepts_dyn(_p: &dyn scavenger::plugins::Plugin) {}
}

#[tokio::test]
#[ignore] // Requires Chrome running with --remote-debugging-port=9222
async fn browser_connect_and_new_page() {
    let page = scavenger::plugins::browser::new_page()
        .await
        .expect("failed to open page");
    page.close().await.expect("failed to close page");
}
