use mongodb::{
    options::{ClientOptions, CreateIndexOptions, IndexOptions},
    Client, Database, IndexModel,
};
use mongodb::bson::{doc, Document}; 
use std::env;

pub struct MongoRepo {
    db: Database,
}

impl MongoRepo {
    pub async fn init() -> Self {
        let uri = env::var("MONGO_URI").expect("MONGO_URI not set");
        let client_options = ClientOptions::parse(uri).await.unwrap();
        let client = Client::with_options(client_options).unwrap();
        let db = client.database("aydin_chat");

        let repo = MongoRepo { db };
        repo.create_indexes().await;
        repo
    }

    async fn create_indexes(&self) {
        let email_otps_col = self.db.collection::<Document>("email_otps");
        let otp_index = IndexModel::builder()
            .keys(doc! { "email": 1 })
            .build();

        let _ = email_otps_col.create_index(otp_index, None::<CreateIndexOptions>).await;

        let users_col = self.db.collection::<Document>("users");
        let user_email_index = IndexModel::builder()
            .keys(doc! { "email": 1 })
            .build();

        let _ = users_col.create_index(user_email_index, None::<CreateIndexOptions>).await;

        let username_index_options = IndexOptions::builder()
            .unique(Some(true))
            .sparse(Some(true))
            .build();
        let user_username_index = IndexModel::builder()
            .keys(doc! { "username": 1 })
            .options(username_index_options)
            .build();

        let _ = users_col
            .create_index(user_username_index, None::<CreateIndexOptions>)
            .await;

        let friendships_col = self.db.collection::<Document>("friendships");
        let friendship_pair_index_options = IndexOptions::builder()
            .unique(Some(true))
            .build();
        let friendship_pair_index = IndexModel::builder()
            .keys(doc! { "user_a": 1, "user_b": 1 })
            .options(friendship_pair_index_options)
            .build();

        let _ = friendships_col
            .create_index(friendship_pair_index, None::<CreateIndexOptions>)
            .await;

        let chat_groups_col = self.db.collection::<Document>("chat_groups");
        let group_creator_index = IndexModel::builder()
            .keys(doc! { "created_by": 1 })
            .build();
        let _ = chat_groups_col
            .create_index(group_creator_index, None::<CreateIndexOptions>)
            .await;

        let group_members_col = self.db.collection::<Document>("group_members");
        let group_member_unique_options = IndexOptions::builder()
            .unique(Some(true))
            .build();
        let group_member_unique = IndexModel::builder()
            .keys(doc! { "group_id": 1, "username": 1 })
            .options(group_member_unique_options)
            .build();
        let _ = group_members_col
            .create_index(group_member_unique, None::<CreateIndexOptions>)
            .await;

        let group_member_by_user = IndexModel::builder()
            .keys(doc! { "username": 1 })
            .build();
        let _ = group_members_col
            .create_index(group_member_by_user, None::<CreateIndexOptions>)
            .await;

        
        println!("MongoDB indexes checked.");
    }

    pub fn get_db(&self) -> &Database {
        &self.db
    }
}