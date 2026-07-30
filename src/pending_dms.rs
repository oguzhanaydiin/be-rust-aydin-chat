use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use futures::TryStreamExt;
use mongodb::bson::{doc, to_bson, DateTime as BsonDateTime};
use mongodb::options::FindOptions;
use mongodb::Database;
use serde::{Deserialize, Serialize};

use crate::models::PendingMessage;

const COLLECTION: &str = "pending_dms";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PendingDmDoc {
    #[serde(rename = "_id")]
    id: String,
    from_username: String,
    to_username: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_data_url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    reactions: HashMap<String, Vec<String>>,
    created_at: BsonDateTime,
}

impl From<&PendingMessage> for PendingDmDoc {
    fn from(message: &PendingMessage) -> Self {
        Self {
            id: message.id.clone(),
            from_username: message.from_username.clone(),
            to_username: message.to_username.clone(),
            text: message.text.clone(),
            image_data_url: message.image_data_url.clone(),
            reactions: message.reactions.clone(),
            created_at: BsonDateTime::from_millis(message.created_at.timestamp_millis()),
        }
    }
}

impl From<PendingDmDoc> for PendingMessage {
    fn from(doc: PendingDmDoc) -> Self {
        let created_at = Utc
            .timestamp_millis_opt(doc.created_at.timestamp_millis())
            .single()
            .unwrap_or_else(Utc::now);

        Self {
            id: doc.id,
            from_username: doc.from_username,
            to_username: doc.to_username,
            text: doc.text,
            image_data_url: doc.image_data_url,
            reactions: doc.reactions,
            created_at,
        }
    }
}

pub async fn insert_pending_dm(db: &Database, message: &PendingMessage) -> Result<(), String> {
    let collection = db.collection::<PendingDmDoc>(COLLECTION);
    collection
        .insert_one(PendingDmDoc::from(message), None)
        .await
        .map_err(|err| format!("failed to persist pending DM: {err}"))?;
    Ok(())
}

pub async fn load_all_pending(db: &Database) -> Result<Vec<PendingMessage>, String> {
    let collection = db.collection::<PendingDmDoc>(COLLECTION);
    let options = FindOptions::builder()
        .sort(doc! { "created_at": 1 })
        .build();

    let cursor = collection
        .find(None, options)
        .await
        .map_err(|err| format!("failed to load pending DMs: {err}"))?;

    let docs: Vec<PendingDmDoc> = cursor
        .try_collect()
        .await
        .map_err(|err| format!("failed to read pending DMs: {err}"))?;

    Ok(docs.into_iter().map(PendingMessage::from).collect())
}

pub async fn ack_pending_dms(
    db: &Database,
    to_username: &str,
    message_ids: &[String],
) -> Result<usize, String> {
    if message_ids.is_empty() {
        return Ok(0);
    }

    let collection = db.collection::<PendingDmDoc>(COLLECTION);
    let result = collection
        .delete_many(
            doc! {
                "to_username": to_username,
                "_id": { "$in": message_ids },
            },
            None,
        )
        .await
        .map_err(|err| format!("failed to ack pending DMs: {err}"))?;

    Ok(result.deleted_count as usize)
}

pub async fn set_pending_dm_reactions(
    db: &Database,
    message_id: &str,
    reactions: &HashMap<String, Vec<String>>,
) -> Result<(), String> {
    let collection = db.collection::<PendingDmDoc>(COLLECTION);
    let reactions_bson =
        to_bson(reactions).map_err(|err| format!("failed to encode reactions: {err}"))?;

    collection
        .update_one(
            doc! { "_id": message_id },
            doc! { "$set": { "reactions": reactions_bson } },
            None,
        )
        .await
        .map_err(|err| format!("failed to persist DM reactions: {err}"))?;

    Ok(())
}
