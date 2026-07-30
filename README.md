# be-rust-aydin-chat

Rust backend for Aydin Chat (OTP auth + WebSocket relay).

Chat design (Signal-like):
- Message history is client-owned.
- Server relays messages in real time via WebSocket.
- Offline messages stay in an **in-memory** mailbox until client ack (lost on restart).
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

- Chat message history is not stored in MongoDB.
- In-memory offline queue is a V0 contract (not durable across restart).
- OTP plaintext is never stored; rate limits apply on send/validate.
- WS images are capped (~512KB); reconnect delivers pending messages one frame at a time.
