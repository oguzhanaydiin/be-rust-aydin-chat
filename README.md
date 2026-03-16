dotenv is for reading .env file
# be-rust-aydin-chat

Rust backend for chat + OTP.

Chat design goal is Signal-like:
- Message history is client-owned.
- Server relays messages in real time via WebSocket.
- Server keeps temporary offline messages in memory until client ack.
- OTP data is stored in MongoDB.

## Project Structure

```text
src/
	main.rs
	app_state.rs
	db.rs
	models.rs
	routes.rs
	handlers/
		mod.rs
		health.rs
		otp.rs
		users.rs
		ws.rs
```

File responsibilities:

- `src/main.rs`
	- Starts Actix server (`127.0.0.1:8080`).
	- Initializes MongoDB connection.
	- Builds shared `AppState` (Mongo DB handle + in-memory mailboxes + online users).
	- Registers all routes.

- `src/app_state.rs`
	- Global runtime state shared across handlers.
	- Holds in-memory chat mailbox per user.
	- Holds online WebSocket connections per user.
	- Provides helper methods:
		- register/unregister socket connection
		- queue message
		- read inbox
		- ack messages
		- dispatch to online user(s)
		- broadcast online-user list

- `src/db.rs`
	- MongoDB initialization and index setup.
	- Currently used for OTP persistence (`email_otps` collection).

- `src/models.rs`
	- Request/response DTOs and shared models.
	- OTP models (`SendEmailOtpRequest`, `ValidateEmailOtpRequest`, `AuthSessionResponse`, etc.).
	- User models (`User`, `SaveUsernameRequest`, `SaveUsernameResponse`).
	- Chat models (`PendingMessage` - used internally by WebSocket handlers).
	- WebSocket event enums (`WsClientEvent`, `WsServerEvent`).

- `src/routes.rs`
	- Central route map for HTTP and WebSocket endpoints.

- `src/handlers/health.rs`
	- Health check endpoint.

- `src/handlers/otp.rs`
	- OTP generation, storage, validation.
	- Uses MongoDB collection `email_otps`.

- `src/handlers/users.rs`
	- User profile management (username).
	- Uses MongoDB collection `users`.

- `src/handlers/ws.rs`
	- WebSocket session lifecycle and event handling.
	- Handles register/send_message/ack/get_online_users events.
	- Manages in-memory message queue for offline delivery.
	- Delivers realtime messages and pushes inbox/online updates.

- `src/handlers/mod.rs`
	- Re-exports handler modules.

## Routes

HTTP routes:

- `GET /health`
	- Health check endpoint.

- `PUT /users/username`
	- Save/update username for authenticated user.
	- Requires Bearer token in Authorization header.
	- Validates username uniqueness.

- `POST /otp/send`
	- Creates OTP and stores it in MongoDB.
	- Returns OTP in dev environment for testing.

- `POST /otp/validate`
	- Validates OTP and returns authentication token.
	- Returns user profile including username if set.

WebSocket route:

- `GET /ws`
	- WebSocket endpoint for realtime chat events.
	- Handles: register, send_message, ack, get_online_users events.

## Dependencies (Cargo.toml)

- `actix-web`: HTTP server and routing
- `actix`, `actix-web-actors`: WebSocket actor/session handling
- `mongodb`: OTP data persistence
- `serde`, `serde_json`: serialization/deserialization
- `tokio`, `tokio-stream`: async runtime and stream integration
- `futures`: async utilities
- `dotenv`: environment variable loading
- `chrono`: UTC timestamp handling for chat messages
- `rand`: OTP code generation

## Notes

- Chat messages are not persisted in MongoDB.
- Offline chat queue is currently in-memory (lost on server restart).
- For stronger delivery guarantees, consider Redis for temporary undelivered messages.
- OTP is returned in `POST /otp/send` response only when `APP_ENV` is set to `dev`, `development`, or `local`.
