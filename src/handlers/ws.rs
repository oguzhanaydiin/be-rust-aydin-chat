use std::time::{Duration, Instant};

use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use chrono::Utc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::app_state::AppState;
use crate::auth::verify_token;
use crate::models::{PendingMessage, WsClientEvent, WsServerEvent};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn ws_index(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (tx, rx) = unbounded_channel::<String>();
    let session = ChatWsSession {
        state: data,
        connection_id: generate_id(),
        user_id: None,
        out_tx: tx,
        out_rx: Some(rx),
        hb: Instant::now(),
    };

    ws::start(session, &req, stream)
}

struct ChatWsSession {
    state: web::Data<AppState>,
    connection_id: String,
    user_id: Option<String>,
    out_tx: UnboundedSender<String>,
    out_rx: Option<UnboundedReceiver<String>>,
    hb: Instant,
}

impl ChatWsSession {
    fn send_error(ctx: &mut ws::WebsocketContext<Self>, message: &str) {
        if let Ok(payload) = serde_json::to_string(&WsServerEvent::Error {
            message: message.to_string(),
        }) {
            ctx.text(payload);
        }
    }

    fn handle_register(&mut self, user_id: String) {
        let normalized = user_id.trim().to_lowercase();
        if normalized.is_empty() {
            return;
        }

        self.user_id = Some(normalized.clone());

        let state = self.state.clone();
        let tx = self.out_tx.clone();
        let connection_id = self.connection_id.clone();

        tokio::spawn(async move {
            state
                .register_connection(&normalized, connection_id, tx.clone())
                .await;

            if let Ok(payload) = serde_json::to_string(&WsServerEvent::Registered {
                user_id: normalized.clone(),
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
                    let _ = state.dispatch_to_user(&msg.from_user_id, &payload).await;
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
        to_user_id: String,
        text: String,
        client_message_id: Option<String>,
    ) {
        let Some(from_user_id) = self.user_id.clone() else {
            return;
        };

        let normalized_to = to_user_id.trim().to_string();
        let normalized_text = text.trim().to_string();

        if normalized_to.is_empty() || normalized_text.is_empty() {
            return;
        }

        let state = self.state.clone();
        let tx = self.out_tx.clone();

        tokio::spawn(async move {
            let message = PendingMessage {
                id: generate_id(),
                from_user_id,
                to_user_id: normalized_to.clone(),
                text: normalized_text,
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
    }

    fn handle_ack(&self, message_ids: Vec<String>) {
        let Some(user_id) = self.user_id.clone() else {
            return;
        };

        let state = self.state.clone();
        let tx = self.out_tx.clone();

        tokio::spawn(async move {
            let removed_count = state.ack_messages(&user_id, &message_ids).await;
            if let Ok(payload) = serde_json::to_string(&WsServerEvent::AckResult { removed_count }) {
                let _ = tx.send(payload);
            }
        });
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
        let Some(user_id) = self.user_id.clone() else {
            return;
        };

        let state = self.state.clone();
        let connection_id = self.connection_id.clone();

        tokio::spawn(async move {
            state.unregister_connection(&user_id, &connection_id).await;
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
                            Ok(claims) => self.handle_register(claims.sub),
                            Err(_) => Self::send_error(ctx, "invalid token"),
                        }
                    }
                    Ok(WsClientEvent::SendMessage {
                        to_user_id,
                        text,
                        client_message_id,
                    }) => {
                        if self.user_id.is_none() {
                            Self::send_error(ctx, "send register event first");
                            return;
                        }

                        if to_user_id.trim().is_empty() || text.trim().is_empty() {
                            Self::send_error(ctx, "to_user_id and text cannot be empty");
                            return;
                        }

                        self.handle_send_message(to_user_id, text, client_message_id);
                    }
                    Ok(WsClientEvent::Ack { message_ids }) => {
                        if self.user_id.is_none() {
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
            Err(_) => {
                ctx.stop();
            }
        }
    }
}

fn generate_id() -> String {
    mongodb::bson::oid::ObjectId::new().to_hex()
}
