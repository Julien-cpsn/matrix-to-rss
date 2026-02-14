use std::time::Duration;
use chrono::Utc;
use log::{error, info, warn};
use matrix_sdk::{reqwest, Client, Room, RoomState};
use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent};
use regex::Regex;
use tokio::time::sleep;
use crate::{Message, CHANNEL_MESSAGES, SUBSCRIBED_CHANNELS};


pub async fn login_and_sync_bot(client: Client) -> anyhow::Result<()> {
    client.add_event_handler(on_stripped_state_member);

    client.account().set_display_name(Some("RSS bot")).await.expect("Couldn't set display name");

    let sync_token = client.sync_once(SyncSettings::default()).await?.next_batch;

    client.add_event_handler(on_room_message);

    let settings = SyncSettings::default().token(sync_token);

    client.sync(settings).await?;

    Ok(())
}

async fn on_stripped_state_member(room_member: StrippedRoomMemberEvent, client: Client, room: Room) {
    if room_member.state_key != client.user_id().unwrap() {
        // the invite we've seen isn't for us, but for someone else. ignore
        return;
    }

    tokio::spawn(async move {
        println!("Autojoining room {}", room.room_id());
        let mut delay = 2;

        while let Err(err) = room.join().await {
            warn!("Failed to join room {} ({err:?}), retrying in {delay}s", room.room_id());

            sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                error!("Can't join room {} ({err:?})", room.room_id());
                break;
            }
        }
        info!("Successfully joined room {}", room.room_id());
    });
}

async fn on_room_message(event: OriginalSyncRoomMessageEvent, room: Room) {
    if room.state() != RoomState::Joined {
        return;
    }

    let MessageType::Text(text_content) = &event.content.msgtype else {
        return;
    };

    if text_content.body.starts_with("!rss") {
        let command = text_content.body.split(' ').collect::<Vec<&str>>();
        handle_command(room, command).await;
    }
    else {
        handle_message(room, event).await;
    }
}

async fn handle_command(room: Room, command: Vec<&str>) {
    const COMMANDS: &str = "Accepted commands are: subscribe, unsubscribe, list";

    if command.len() != 2 {
        room.send(RoomMessageEventContent::text_plain(COMMANDS));
        return;
    }

    let response: RoomMessageEventContent;

    let room_id = room.room_id().to_string();
    let room_name = match room.name() {
        Some(room_name) => room_name[1..].to_string(), // Remove first #
        None => {
            room.send(RoomMessageEventContent::text_plain("Please set a room name first"));
            return;
        }
    };

    match command[1] {
        "subscribe" => {
            let mut subscribed_channels = SUBSCRIBED_CHANNELS.write();
            let mut channel_messages = CHANNEL_MESSAGES.write();

            if subscribed_channels.contains_key(&room_id) {
                response = RoomMessageEventContent::text_plain(format!("Already subscribed to room \"{}\"", &room_name));
            }
            else {
                response = RoomMessageEventContent::text_plain(format!("Successfully subscribed to room \"{}\"", &room_name));
                subscribed_channels.insert(room_id, room_name.clone());
                channel_messages.insert(room_name, Vec::new());
            }
        },
        "unsubscribe" => {
            let mut subscribed_channels = SUBSCRIBED_CHANNELS.write();
            let mut channel_messages = CHANNEL_MESSAGES.write();

            if subscribed_channels.contains_key(&room_id) {
                response = RoomMessageEventContent::text_plain(format!("Successfully subscribed to room \"{}\"", &room_name));
                subscribed_channels.remove(&room_id);
                channel_messages.remove(&room_name);
            }
            else {
                response = RoomMessageEventContent::text_plain(format!("Already unsubscribed from room \"{}\"", &room_name));
            }
        },
        "list" => {
            let subscribed_channels = SUBSCRIBED_CHANNELS.read();
            let mut text = String::from("Subscribed to:\n");

            for (key, name) in subscribed_channels.iter() {
                text.push_str(&format!("\t- {} ({})", name, key));
            }

            response = RoomMessageEventContent::text_plain(text);
        }
        "help" | _ => response = RoomMessageEventContent::text_plain(COMMANDS)
    }

    room.send(response).await.unwrap();
}

async fn handle_message(room: Room, event: OriginalSyncRoomMessageEvent) {
    let MessageType::Text(text_content) = &event.content.msgtype else {
        return;
    };

    let text_content = text_content.body.to_string();

    let room_name = match room.name() {
        Some(room_name) => room_name[1..].to_string(), // Remove first #
        None => {
            room.send(RoomMessageEventContent::text_plain("Please set a room name first"));
            return;
        }
    };

    if !CHANNEL_MESSAGES.read().contains_key(&room_name) {
        return;
    }

    let mut channel_messages = CHANNEL_MESSAGES.write();
    let messages = channel_messages.get_mut(&room_name).unwrap();

    let url_regex = Regex::new(r"(?<url>https?://(www\.)?[-a-zA-Z0-9@:%._+~#=]{1,256}\.[a-zA-Z0-9()]{1,6}\b([-a-zA-Z0-9()@:%_+.~#?&/=]*))").unwrap();

    if let Some(captures) = url_regex.captures(&text_content) {
        if captures.len() == 0 {
            return;
        }

        let sender = event.sender.to_string();
        let link = captures.get(1).unwrap().as_str().to_string();
        let mut page_name = None;

        if let Ok(res) = reqwest::get(&link).await {
            let text = res.text().await.unwrap();

            let title_regex = Regex::new(r"<title>(?<title>.*)</title>").unwrap();

            if let Some(captures) = title_regex.captures(&text) {
                if captures.len() == 0 {
                    return;
                }

                page_name = Some(captures.get(1).unwrap().as_str().to_string());
            }
        }

        messages.truncate(50);

        messages.insert(0, Message {
            sender,
            content: text_content.to_owned(),
            link,
            page_name,
            time: Utc::now(),
        });
    }
}