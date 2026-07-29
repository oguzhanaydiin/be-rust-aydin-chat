use std::collections::HashMap;
use std::time::Duration;

use chat_api::app_state::AppState;
use chat_api::auth::DEFAULT_JWT_TTL_SECONDS;
use chat_api::handlers::ws::{MAX_WS_FRAME_SIZE, MAX_WS_IMAGE_DATA_URL_BYTES};
use chat_api::models::{PendingMessage, WsServerEvent};
use chat_api::otp_limit::OtpRateLimiter;
use chrono::Utc;
use mongodb::{
    options::{ClientOptions, ServerAddress},
    Client,
};
use tokio::sync::{mpsc::unbounded_channel, RwLock};

fn test_app_state(database_name: &str) -> AppState {
    let options = ClientOptions::builder()
        .hosts(vec![ServerAddress::Tcp {
            host: "127.0.0.1".to_string(),
            port: Some(1),
        }])
        .server_selection_timeout(Some(Duration::from_millis(50)))
        .build();
    let client = Client::with_options(options).expect("client should build");

    AppState {
        db: client.database(database_name),
        jwt_secret: "test-secret".to_string(),
        jwt_ttl_secs: DEFAULT_JWT_TTL_SECONDS,
        otp_rate_limiter: RwLock::new(OtpRateLimiter::new()),
        mailboxes: RwLock::new(HashMap::new()),
        group_mailboxes: RwLock::new(HashMap::new()),
        message_reactions: RwLock::new(HashMap::new()),
        group_message_reactions: RwLock::new(HashMap::new()),
        group_message_members: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    }
}

fn pending(id: &str, from: &str, to: &str, image: Option<String>) -> PendingMessage {
    PendingMessage {
        id: id.to_string(),
        from_username: from.to_string(),
        to_username: to.to_string(),
        text: if image.is_some() {
            String::new()
        } else {
            "hello".to_string()
        },
        image_data_url: image,
        reactions: HashMap::new(),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn queue_inbox_ack_removes_only_acked_ids() {
    let state = test_app_state("mailbox_ack");
    state
        .queue_message(pending("m1", "alice", "bob", None))
        .await;
    state
        .queue_message(pending("m2", "alice", "bob", None))
        .await;

    let inbox = state.get_inbox("bob").await;
    assert_eq!(inbox.len(), 2);

    let removed = state.ack_messages("bob", &["m1".to_string()]).await;
    assert_eq!(removed, 1);

    let remaining = state.get_inbox("bob").await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "m2");
}

#[tokio::test]
async fn dispatch_to_offline_user_returns_zero_and_keeps_mailbox() {
    let state = test_app_state("mailbox_offline");
    state
        .queue_message(pending("m1", "alice", "bob", None))
        .await;

    let delivered = state.dispatch_to_user("bob", "payload").await;
    assert_eq!(delivered, 0);
    assert_eq!(state.get_inbox("bob").await.len(), 1);
}

#[tokio::test]
async fn dispatch_to_live_connection_counts_and_prunes_dead_tx() {
    let state = test_app_state("mailbox_dispatch");
    let (live_tx, mut live_rx) = unbounded_channel();
    let (dead_tx, dead_rx) = unbounded_channel();
    drop(dead_rx);

    state
        .register_connection("bob", "live".to_string(), live_tx)
        .await;
    state
        .register_connection("bob", "dead".to_string(), dead_tx)
        .await;

    let delivered = state.dispatch_to_user("bob", "hello").await;
    assert_eq!(delivered, 1);
    assert_eq!(live_rx.try_recv().expect("live should receive"), "hello");
    assert_eq!(state.online_user_ids().await, vec!["bob".to_string()]);
}

#[test]
fn single_max_image_new_message_fits_ws_frame() {
    let image = format!("data:image/png;base64,{}", "A".repeat(MAX_WS_IMAGE_DATA_URL_BYTES - 22));
    assert!(image.len() <= MAX_WS_IMAGE_DATA_URL_BYTES);

    let payload = serde_json::to_string(&WsServerEvent::NewMessage {
        message: pending("img-1", "alice", "bob", Some(image)),
    })
    .expect("serialize");

    assert!(
        payload.len() <= MAX_WS_FRAME_SIZE,
        "single image NewMessage must fit frame ({} > {})",
        payload.len(),
        MAX_WS_FRAME_SIZE
    );
}

#[test]
fn bulk_inbox_of_two_max_images_exceeds_ws_frame() {
    let image = format!("data:image/png;base64,{}", "B".repeat(MAX_WS_IMAGE_DATA_URL_BYTES - 22));
    let messages = vec![
        pending("img-1", "alice", "bob", Some(image.clone())),
        pending("img-2", "alice", "bob", Some(image)),
    ];

    let payload = serde_json::to_string(&WsServerEvent::Inbox { messages }).expect("serialize");
    assert!(
        payload.len() > MAX_WS_FRAME_SIZE,
        "bulk Inbox of two max images should exceed frame so reconnect must chunk"
    );
}
