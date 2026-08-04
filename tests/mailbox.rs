use std::collections::HashMap;
use std::time::Duration;

use chat_api::app_state::AppState;
use chat_api::auth::DEFAULT_JWT_TTL_SECONDS;
use chat_api::handlers::ws::{MAX_WS_FRAME_SIZE, MAX_WS_IMAGE_DATA_URL_BYTES};
use chat_api::models::{GroupPendingMessage, PendingMessage, WsServerEvent};
use chat_api::otp_limit::OtpRateLimiter;
use chrono::Utc;
use mongodb::{
    options::{ClientOptions, ServerAddress},
    Client, Database,
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
        persist_dms: false,
        otp_rate_limiter: RwLock::new(OtpRateLimiter::new()),
        mailboxes: RwLock::new(HashMap::new()),
        group_mailboxes: RwLock::new(HashMap::new()),
        message_reactions: RwLock::new(HashMap::new()),
        group_message_reactions: RwLock::new(HashMap::new()),
        group_message_members: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    }
}

fn durable_app_state(db: Database) -> AppState {
    AppState {
        db,
        jwt_secret: "test-secret".to_string(),
        jwt_ttl_secs: DEFAULT_JWT_TTL_SECONDS,
        persist_dms: true,
        otp_rate_limiter: RwLock::new(OtpRateLimiter::new()),
        mailboxes: RwLock::new(HashMap::new()),
        group_mailboxes: RwLock::new(HashMap::new()),
        message_reactions: RwLock::new(HashMap::new()),
        group_message_reactions: RwLock::new(HashMap::new()),
        group_message_members: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    }
}

async fn test_mongo_db(database_name: &str) -> Option<Database> {
    let uri = std::env::var("TEST_MONGO_URI").ok()?;
    let client = Client::with_uri_str(&uri).await.ok()?;
    let db = client.database(database_name);
    let _ = db.drop(None).await;
    Some(db)
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
async fn durable_mailbox_survives_rehydrate_like_restart() {
    let Some(db) = test_mongo_db("aydin_chat_test_durable_mailbox").await else {
        eprintln!("skip durable_mailbox_survives_rehydrate_like_restart: set TEST_MONGO_URI");
        return;
    };

    let writer = durable_app_state(db.clone());
    writer
        .queue_message(pending("m1", "alice", "bob", None))
        .await
        .expect("persist m1");
    writer
        .queue_message(pending("m2", "alice", "bob", None))
        .await
        .expect("persist m2");
    writer
        .toggle_message_reaction("m1", "heart", "bob")
        .await;

    // Fresh process: empty memory, hydrate from Mongo.
    let restarted = durable_app_state(db.clone());
    let hydrated = restarted
        .hydrate_pending_dms()
        .await
        .expect("hydrate after restart");
    assert_eq!(hydrated, 2);

    let inbox = restarted.get_inbox("bob").await;
    assert_eq!(inbox.len(), 2);
    assert_eq!(inbox[0].id, "m1");
    assert_eq!(
        inbox[0].reactions.get("heart"),
        Some(&vec!["bob".to_string()])
    );

    let removed = restarted.ack_messages("bob", &["m1".to_string()]).await;
    assert_eq!(removed, 1);

    let after_ack = durable_app_state(db);
    let hydrated_again = after_ack.hydrate_pending_dms().await.expect("hydrate");
    assert_eq!(hydrated_again, 1);
    let remaining = after_ack.get_inbox("bob").await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "m2");
}

#[tokio::test]
async fn queue_inbox_ack_removes_only_acked_ids() {
    let state = test_app_state("mailbox_ack");
    state
        .queue_message(pending("m1", "alice", "bob", None))
        .await
        .expect("queue m1");
    state
        .queue_message(pending("m2", "alice", "bob", None))
        .await
        .expect("queue m2");

    let inbox = state.get_inbox("bob").await;
    assert_eq!(inbox.len(), 2);

    let removed = state.ack_messages("bob", &["m1".to_string()]).await;
    assert_eq!(removed, 1);

    let remaining = state.get_inbox("bob").await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "m2");
}

#[tokio::test]
async fn dm_ack_clears_message_reactions() {
    let state = test_app_state("mailbox_ack_reactions");
    state
        .queue_message(pending("m1", "alice", "bob", None))
        .await
        .expect("queue m1");
    state.toggle_message_reaction("m1", "heart", "bob").await;

    assert!(state.message_reactions.read().await.contains_key("m1"));

    let removed = state.ack_messages("bob", &["m1".to_string()]).await;
    assert_eq!(removed, 1);
    assert!(!state.message_reactions.read().await.contains_key("m1"));
}

fn group_pending(id: &str, group_id: &str, from: &str) -> GroupPendingMessage {
    GroupPendingMessage {
        id: id.to_string(),
        group_id: group_id.to_string(),
        from_username: from.to_string(),
        text: "hello".to_string(),
        image_data_url: None,
        reactions: HashMap::new(),
        created_at: Utc::now(),
    }
}

#[tokio::test]
async fn group_ack_clears_maps_only_when_all_recipients_ack() {
    let state = test_app_state("mailbox_group_ack_maps");
    let recipients = vec!["bob".to_string(), "carol".to_string()];
    state
        .queue_group_message(group_pending("g1", "group-1", "alice"), &recipients)
        .await;
    state
        .toggle_group_message_reaction("g1", "thumbsup", "bob")
        .await;

    let removed_bob = state.ack_group_messages("bob", &["g1".to_string()]).await;
    assert_eq!(removed_bob, 1);
    assert!(state.group_message_members.read().await.contains_key("g1"));
    assert!(state.group_message_reactions.read().await.contains_key("g1"));
    assert_eq!(
        state.group_message_recipients("g1").await,
        vec!["carol".to_string()]
    );

    let removed_carol = state.ack_group_messages("carol", &["g1".to_string()]).await;
    assert_eq!(removed_carol, 1);
    assert!(!state.group_message_members.read().await.contains_key("g1"));
    assert!(!state.group_message_reactions.read().await.contains_key("g1"));
}

#[tokio::test]
async fn dispatch_to_offline_user_returns_zero_and_keeps_mailbox() {
    let state = test_app_state("mailbox_offline");
    state
        .queue_message(pending("m1", "alice", "bob", None))
        .await
        .expect("queue m1");

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
