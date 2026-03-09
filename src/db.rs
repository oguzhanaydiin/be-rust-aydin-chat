use mongodb::{options::ClientOptions, options::CreateIndexOptions, Client, Database, IndexModel};
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
        
        println!("MongoDB indexes checked.");
    }

    pub fn get_db(&self) -> &Database {
        &self.db
    }
}