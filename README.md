# be-rust-aydin-chat

Rust backend for Aydin Chat (OTP auth + WebSocket relay).

Chat design (Signal-like):
- Message history is client-owned.
- Server relays messages in real time via WebSocket.
- Offline DMs stay in a Mongo-backed mailbox (`pending_dms`) until client ack (survives restart).
- OTP + users live in MongoDB; OTPs are stored hashed.

## How to run

1. MongoDB running and reachable.
2. Create `.env` in this repo:

```env
MONGO_URI=mongodb://127.0.0.1:27017/aydin_chat
JWT_SECRET=change-me
RESEND_API_KEY=...
RESEND_FROM_EMAIL=...
APP_ENV=dev
```

Optional: `JWT_TTL_SECONDS` (default 7 days).

3. Start the API:

```bash
cargo run
```

Listens on `http://127.0.0.1:8080` and `ws://127.0.0.1:8080/ws`.

4. Check health: `GET http://127.0.0.1:8080/health`

5. Pair with the frontend (`fe-aydin-chat`):

```bash
npm install
npm run dev
```

Defaults: `NEXT_PUBLIC_API_URL=http://127.0.0.1:8080`, `NEXT_PUBLIC_WS_URL=ws://127.0.0.1:8080/ws`.

With `APP_ENV=dev`, `POST /otp/send` also returns the OTP in the JSON body (handy for local login without opening mail).

## Try a chat

1. Two browsers (or normal + incognito).
2. Login with two emails via OTP; set usernames.
3. Send friend request → accept.
4. DM text / small image; try offline peer then reconnect.

## Routes (main)

- `GET /health`
- `POST /otp/send`, `POST /otp/validate`
- `PUT /users/username`, `GET /users/me`, friends + groups HTTP APIs
- `GET /ws` — register, send_message, ack, reactions, online users

## Notes

- Chat message history is not stored in MongoDB (only undelivered DMs until ack).
- DM offline queue is durable in `pending_dms` and hydrated on boot; group offline queues stay in-memory.
- OTP plaintext is never stored. Send limit: 3/email/15m (counted only after a successful email send) plus ~10/IP/15m; validate: 8/email/15m plus ~20/IP/15m. Limits are in-memory only and reset on process restart.
- WS images are capped (~512KB); reconnect delivers pending messages one frame at a time.
- Durable mailbox integration test: set `TEST_MONGO_URI` then `cargo test durable_mailbox`.
