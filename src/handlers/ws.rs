use std::time::{Duration, Instant};
use std::collections::HashMap;

use actix::{Actor, ActorContext, ActorFutureExt, AsyncContext, StreamHandler, WrapFuture};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use chrono::Utc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::app_state::AppState;
use crate::auth::verify_token;
use crate::models::{PendingMessage, User, WsClientEvent, WsServerEvent};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_WS_FRAME_SIZE: usize = 32 * 1024 * 1024;

pub async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (tx, rx) = unbounded_channel::<String>();
    let session = ChatWsSession {
        state: data,
        connection_id: generate_id(),
        username: None,
        out_tx: tx,
        out_rx: Some(rx),
        hb: Instant::now(),
    };

    ws::WsResponseBuilder::new(session, &req, stream)
        .frame_size(MAX_WS_FRAME_SIZE)
        .start()
}

struct ChatWsSession {
    state: web::Data<AppState>,
    connection_id: String,
    username: Option<String>,
    out_tx: UnboundedSender<String>,
    out_rx: Option<UnboundedReceiver<String>>,
    hb: Instant,
}

impl ChatWsSession {
    async fn resolve_username_by_email(
        state: web::Data<AppState>,
        email: String,
    ) -> Result<String, String> {
        let users_col = state.db.collection::<User>("users");
        let found_user = users_col
            .find_one(mongodb::bson::doc! { "email": &email }, None)
            .await
            .map_err(|_| "failed to fetch user".to_string())?;

        let username = found_user
            .and_then(|u| u.username)
            .ok_or_else(|| "username not found for this account".to_string())?;

        let normalized = username.trim().to_lowercase();
        if normalized.is_empty() {
            return Err("username is invalid for this account".to_string());
        }

        Ok(normalized)
    }

    fn send_error_with_metadata(
        ctx: &mut ws::WebsocketContext<Self>,
        message: &str,
        client_message_id: Option<String>,
        message_id: Option<String>,
    ) {
        if let Ok(payload) = serde_json::to_string(&WsServerEvent::Error {
            message: message.to_string(),
            client_message_id,
            message_id,
        }) {
            ctx.text(payload);
        }
    }

    fn send_error(ctx: &mut ws::WebsocketContext<Self>, message: &str) {
        Self::send_error_with_metadata(ctx, message, None, None);
    }

    fn handle_register(&mut self, username: String) {
        let normalized = username.trim().to_lowercase();
        if normalized.is_empty() {
            return;
        }

        self.username = Some(normalized.clone());

        let state = self.state.clone();
        let tx = self.out_tx.clone();
        let connection_id = self.connection_id.clone();

        tokio::spawn(async move {
            state
                .register_connection(&normalized, connection_id, tx.clone())
                .await;

            if let Ok(payload) = serde_json::to_string(&WsServerEvent::Registered {
                username: normalized.clone(),
            }) {
                let _ = tx.send(payload);
            }

            let inbox = state.get_inbox(&normalized).await;

            // Notify senders of queued messages that their message is now delivered
            for msg in &inbox {
                if let Ok(payload) = serde_json::to_string(&WsServerEvent::MessageDelivered {
                    message_id: msg.id.clone(),
                    client_message_id: None,
                }) {
                    let _ = state.dispatch_to_user(&msg.from_username, &payload).await;
                }
            }

            if let Ok(payload) = serde_json::to_string(&WsServerEvent::Inbox { messages: inbox }) {
                let _ = tx.send(payload);
            }

            let online_users = state.online_user_ids().await;
            if let Ok(payload) = serde_json::to_string(&WsServerEvent::OnlineUsers {
                users: online_users,
            }) {
                state.broadcast_to_all_online(&payload).await;
            }
        });
    }

    fn handle_send_message(
        &self,
        to_username: String,
        text: String,
        image_data_url: Option<String>,
        client_message_id: Option<String>,
    ) -> Result<(), String> {
        let Some(from_username) = self.username.clone() else {
            return Err("send register event first".to_string());
        };

        let normalized_to = to_username.trim().to_lowercase();
        let normalized_text = text.trim().to_string();
        let normalized_image_data_url = image_data_url.and_then(|raw| {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        if normalized_to.is_empty()
            || (normalized_text.is_empty() && normalized_image_data_url.is_none())
        {
            return Err("to_username and at least one content field are required".to_string());
        }

        if let Some(image) = &normalized_image_data_url {
            const MAX_IMAGE_DATA_URL_BYTES: usize = 6 * 1024 * 1024;
            if !image.starts_with("data:image/") || image.len() > MAX_IMAGE_DATA_URL_BYTES {
                return Err("image payload is invalid or too large".to_string());
            }
        }

        if normalized_text.len() > 4000 {
            return Err("text is too long".to_string());
        }

        let state = self.state.clone();
        let tx = self.out_tx.clone();

        tokio::spawn(async move {
            let message = PendingMessage {
                id: generate_id(),
                from_username,
                to_username: normalized_to.clone(),
                text: normalized_text,
                image_data_url: normalized_image_data_url,
                reactions: HashMap::new(),
                created_at: Utc::now(),
            };

            state.queue_message(message.clone()).await;

            let delivered = if let Ok(payload) = serde_json::to_string(&WsServerEvent::NewMessage {
                message: message.clone(),
            }) {
                state.dispatch_to_user(&normalized_to, &payload).await
            } else {
                0
            };

            if let Ok(payload) = serde_json::to_string(&WsServerEvent::MessageQueued {
                message_id: message.id.clone(),
                client_message_id: client_message_id.clone(),
            }) {
                let _ = tx.send(payload);
            }

            if delivered > 0 {
                if let Ok(payload) = serde_json::to_string(&WsServerEvent::MessageDelivered {
                    message_id: message.id,
                    client_message_id,
                }) {
                    let _ = tx.send(payload);
                }
            }
        });

        Ok(())
    }

    fn handle_ack(&self, message_ids: Vec<String>) {
        let Some(username) = self.username.clone() else {
            return;
        };

        let state = self.state.clone();
        let tx = self.out_tx.clone();

        tokio::spawn(async move {
            let removed_count = state.ack_messages(&username, &message_ids).await;
            if let Ok(payload) = serde_json::to_string(&WsServerEvent::AckResult { removed_count }) {
                let _ = tx.send(payload);
            }
        });
    }

    fn handle_react_message(
        &self,
        message_id: String,
        to_username: String,
        reaction: String,
    ) -> Result<(), String> {
        let Some(by_username) = self.username.clone() else {
            return Err("send register event first".to_string());
        };

        let normalized_message_id = message_id.trim().to_string();
        let normalized_to = to_username.trim().to_lowercase();
        let normalized_reaction = reaction.trim().to_string();

        if normalized_message_id.is_empty() || normalized_to.is_empty() || normalized_reaction.is_empty() {
            return Err("message_id, to_username and reaction are required".to_string());
        }

        let state = self.state.clone();

        tokio::spawn(async move {
            let reactions = state
                .toggle_message_reaction(&normalized_message_id, &normalized_reaction, &by_username)
                .await;

            if let Ok(payload) = serde_json::to_string(&WsServerEvent::MessageReactionsUpdated {
                message_id: normalized_message_id.clone(),
                reactions,
            }) {
                let _ = state.dispatch_to_user(&by_username, &payload).await;
                if normalized_to != by_username {
                    let _ = state.dispatch_to_user(&normalized_to, &payload).await;
                }
            }
        });

        Ok(())
    }
}

impl Actor for ChatWsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb = Instant::now();

        if let Some(rx) = self.out_rx.take() {
            ctx.add_stream(UnboundedReceiverStream::new(rx));
        }

        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                ctx.stop();
                return;
            }
            ctx.ping(b"ping");
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        let Some(username) = self.username.clone() else {
            return;
        };

        let state = self.state.clone();
        let connection_id = self.connection_id.clone();

        tokio::spawn(async move {
            state.unregister_connection(&username, &connection_id).await;
            let online_users = state.online_user_ids().await;
            if let Ok(payload) = serde_json::to_string(&WsServerEvent::OnlineUsers {
                users: online_users,
            }) {
                state.broadcast_to_all_online(&payload).await;
            }
        });
    }
}

impl StreamHandler<String> for ChatWsSession {
    fn handle(&mut self, item: String, ctx: &mut Self::Context) {
        ctx.text(item);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ChatWsSession {
    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                let event = serde_json::from_str::<WsClientEvent>(&text);
                match event {
                    Ok(WsClientEvent::Register { token }) => {
                        let token = token.trim();
                        if token.is_empty() {
                            Self::send_error(ctx, "token cannot be empty");
                            return;
                        }

                        match verify_token(&self.state.jwt_secret, token) {
                            Ok(claims) => {
                                let email = claims.email.trim().to_lowercase();
                                if email.is_empty() {
                                    Self::send_error(ctx, "invalid token claims");
                                    return;
                                }

                                let state = self.state.clone();
                                let register_future = Self::resolve_username_by_email(state, email)
                                    .into_actor(self)
                                    .map(|result, act, ctx| match result {
                                        Ok(username) => act.handle_register(username),
                                        Err(message) => Self::send_error(ctx, &message),
                                    });

                                ctx.spawn(register_future);
                            }
                            Err(_) => Self::send_error(ctx, "invalid token"),
                        }
                    }
                    Ok(WsClientEvent::SendMessage {
                        to_username,
                        text,
                        image_data_url,
                        client_message_id,
                    }) => {
                        if self.username.is_none() {
                            Self::send_error_with_metadata(
                                ctx,
                                "send register event first",
                                client_message_id.clone(),
                                None,
                            );
                            return;
                        }

                        let has_text = !text.trim().is_empty();
                        let has_image = image_data_url
                            .as_ref()
                            .map(|value| !value.trim().is_empty())
                            .unwrap_or(false);

                        if to_username.trim().is_empty() || (!has_text && !has_image) {
                            Self::send_error_with_metadata(
                                ctx,
                                "to_username and at least one content field are required",
                                client_message_id.clone(),
                                None,
                            );
                            return;
                        }

                        if let Err(message) = self.handle_send_message(
                            to_username,
                            text,
                            image_data_url,
                            client_message_id.clone(),
                        ) {
                            Self::send_error_with_metadata(ctx, &message, client_message_id, None);
                        }
                    }
                    Ok(WsClientEvent::ReactMessage {
                        message_id,
                        to_username,
                        reaction,
                    }) => {
                        if self.username.is_none() {
                            Self::send_error(ctx, "send register event first");
                            return;
                        }

                        if let Err(message) = self.handle_react_message(message_id, to_username, reaction) {
                            Self::send_error(ctx, &message);
                        }
                    }
                    Ok(WsClientEvent::Ack { message_ids }) => {
                        if self.username.is_none() {
                            Self::send_error(ctx, "send register event first");
                            return;
                        }

                        self.handle_ack(message_ids);
                    }
                    Ok(WsClientEvent::GetOnlineUsers) => {
                        let state = self.state.clone();
                        let tx = self.out_tx.clone();
                        tokio::spawn(async move {
                            let users = state.online_user_ids().await;
                            if let Ok(payload) =
                                serde_json::to_string(&WsServerEvent::OnlineUsers { users })
                            {
                                let _ = tx.send(payload);
                            }
                        });
                    }
                    Err(_) => {
                        Self::send_error(ctx, "invalid ws event payload");
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Binary(_)) => {}
            Ok(ws::Message::Continuation(_)) => {}
            Ok(ws::Message::Nop) => {}
            Err(err) => {
                eprintln!("ws protocol error: {err}");
                ctx.stop();
            }
        }
    }
}

fn generate_id() -> String {
    mongodb::bson::oid::ObjectId::new().to_hex()
}
