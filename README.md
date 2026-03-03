cargo.tml file:

[package] part:
name is binary name of project with `cargo build`
edition is 2021 most stabile

[dependencies] part:
actix-web is production ready web framework
mongodb with version, tokio-runtime is for async 
serde serializiation/deserialization library for rust
serde_json is for handling json
tokio is rust async runtime
futures is giving more async tools
dotenv is for reading .env file
bson binary json, used with mongodb _id and date-chrono
chrono is handling date and time
