use scavenger::plugins::PluginError;

#[test]
fn bot_detected_error_formatting() {
    let err = PluginError::BotDetected {
        plugin_id: "ebay".into(),
        url: "https://ebay.com/search".into(),
        message: "CAPTCHA presented".into(),
    };
    let msg = err.to_string();
    assert_eq!(msg, "Bot detected on ebay: CAPTCHA presented");
}

#[test]
fn bot_detected_error_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PluginError>();
}

#[test]
fn plugin_trait_is_object_safe() {
    fn _accepts_dyn(_p: &dyn scavenger::plugins::Plugin) {}
}

#[tokio::test]
#[ignore]
async fn browser_connect_and_new_page() {
    let page = scavenger::plugins::browser::new_page()
        .await
        .expect("failed to open page");
    page.close().await.expect("failed to close page");
}
