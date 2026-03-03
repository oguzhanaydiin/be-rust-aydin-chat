use mongodb::{Client, options::ClientOptions, options::CreateIndexOptions, IndexModel, Database};
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
        let messages_col = self.db.collection::<Document>("messages");
        
        let index_model = IndexModel::builder()
            .keys(doc! { "conversation_id": 1, "created_at": -1 })
            .build();
            
        // Explicitly specifying the type to eliminate error margin
        let _ = messages_col.create_index(index_model, None::<CreateIndexOptions>).await;

        let conversations_col = self.db.collection::<Document>("conversations");
        let conv_index = IndexModel::builder()
            .keys(doc! { "members": 1 })
            .build();
            
        let _ = conversations_col.create_index(conv_index, None::<CreateIndexOptions>).await;
        
        println!("MongoDB indexes checked.");
    }

    pub fn get_db(&self) -> &Database {
        &self.db
    }
}