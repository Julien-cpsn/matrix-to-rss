mod bot;
mod server;

use crate::bot::login_and_sync_bot;
use dotenv::dotenv;
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::{env, process::exit};
use chrono::{DateTime, Utc};
use log::info;
use matrix_sdk::Client;
use crate::server::launch_server;

pub static HOMESERVER_URL: OnceCell<String> = OnceCell::new();
pub static SUBSCRIBED_CHANNELS: Lazy<Arc<RwLock<HashMap<String, String>>>> = Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));
pub static CHANNEL_MESSAGES: Lazy<Arc<RwLock<HashMap<String, Vec<Message>>>>> = Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

#[derive(Debug, Clone)]
pub struct Message {
    pub sender: String,
    pub content: String,
    pub page_name: Option<String>,
    pub link: String,
    pub time: DateTime<Utc>
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    dotenv().expect("Failed to load .env file");

    let mut homeserver_url: Option<String> = None;
    let mut username: Option<String> = None;
    let mut password: Option<String> = None;

    for (key, value) in env::vars() {
        match key.as_str() {
            "HOMESERVER_URL" => homeserver_url = Some(value),
            "BOT_USERNAME" => username = Some(value),
            "BOT_PASSWORD" => password = Some(value),
            _ => {}
        }
    }

    if homeserver_url.is_none() {
        println!("Please set the HOMESERVER_URL");
        exit(1);
    }
    if username.is_none() {
        println!("Please set a BOT_USERNAME");
        exit(1);
    }
    if password.is_none() {
        println!("Please set a BOT_PASSWORD");
        exit(1);
    }

    let homeserver_url = homeserver_url.unwrap();
    let username = username.unwrap();
    let password = password.unwrap();

    HOMESERVER_URL.get_or_init(|| homeserver_url.clone());

    info!("Logging in the bot...");

    let client = Client::builder()
        .homeserver_url(homeserver_url)
        .build()
        .await?;

    client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name("rss bot")
        .await?;

    println!("Bot logged in!");
    info!("Logged in as {username}");

    tokio::spawn(login_and_sync_bot(client));

    let address = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:3006".to_string());
    
    println!("Starting server at {address}");

    launch_server(&address).await?;

    Ok(())
}