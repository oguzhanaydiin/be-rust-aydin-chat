use std::time::Duration;

use chat_api::handlers::friends::are_accepted_friends;
use mongodb::{
    options::{ClientOptions, ServerAddress},
    Client,
};

fn unreachable_db(database_name: &str) -> mongodb::Database {
    let options = ClientOptions::builder()
        .hosts(vec![ServerAddress::Tcp {
            host: "127.0.0.1".to_string(),
            port: Some(1),
        }])
        .server_selection_timeout(Some(Duration::from_millis(50)))
        .build();
    let client = Client::with_options(options).expect("client should build");
    client.database(database_name)
}

#[tokio::test]
async fn stranger_and_self_pairs_are_not_accepted_friends_without_db_lookup() {
    let db = unreachable_db("friend_dm_gate");

    assert_eq!(
        are_accepted_friends(&db, "alice", "alice")
            .await
            .expect("self check should short-circuit"),
        false
    );
    assert_eq!(
        are_accepted_friends(&db, "", "bob")
            .await
            .expect("empty check should short-circuit"),
        false
    );
    assert_eq!(
        are_accepted_friends(&db, "alice", "  ")
            .await
            .expect("blank peer should short-circuit"),
        false
    );
}
