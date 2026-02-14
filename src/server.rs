use std::net::SocketAddr;
use std::str::FromStr;
use chrono::Utc;
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use hyper::http::Error;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use log::{info, warn};
use rss::{Category, ChannelBuilder, ItemBuilder, Source};
use tokio::net::TcpListener;
use crate::{Message, CHANNEL_MESSAGES, HOMESERVER_URL};

pub async fn launch_server(address: &str) -> anyhow::Result<()> {
    let addr = SocketAddr::from_str(&address)?;

    info!("Listening on {address}...");
    let listener = TcpListener::bind(addr).await?;

    info!("Server started");
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::task::spawn(async move {
            match http1::Builder::new().serve_connection(io, service_fn(send_rss)).await {
                Ok(_) => (),
                Err(err) => warn!("Error serving connection: {:?}", err)
            }
        });
    }
}

async fn send_rss(request: Request<Incoming>) -> Result<Response<Full<Bytes>>, Error> {
    if request.method() != Method::GET {
        let mut not_allowed = Response::default();
        *not_allowed.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
        return Ok(not_allowed);
    }

    let path = request.uri().path().split('/').collect::<Vec<&str>>();

    let channel_messages = CHANNEL_MESSAGES.read();
    let decoded_path = urlencoding::decode(path[1]).unwrap();

    if path.len() != 2 || !channel_messages.contains_key(decoded_path.as_ref()) {
        let mut not_found = Response::default();
        *not_found.status_mut() = StatusCode::NOT_FOUND;
        return Ok(not_found);
    }

    let (name, messages) = channel_messages
        .get_key_value(decoded_path.as_ref())
        .map(|(key, value)| (key.clone(), value.clone()))
        .unwrap();

    drop(channel_messages);

    let rss = build_rss(name, messages);

    let response = Response::builder()
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("Access-Control-Allow-Origin", "*")
        .status(StatusCode::OK)
        .body(Full::new(Bytes::from(rss)))?;

    Ok(response)
}

fn build_rss(name: String, messages: Vec<Message>) -> String {
    let mut channel = ChannelBuilder::default()
        .title(format!("{name} messages"))
        .description(format!("An RSS feed for {} matrix channel messages", name))
        .language(String::from("en-US"))
        .generator(String::from(env!("CARGO_PKG_NAME")))
        .ttl(String::from("60"))
        .docs(String::from("https://cyber.harvard.edu/rss/rss.html"))
        .categories(vec![Category::from("Matrix")])
        .build();

    let mut items = vec![];

    let homeserver_url = HOMESERVER_URL.get().unwrap().clone();

    for message in messages {
        let title = message.page_name.unwrap_or(message.content.clone());

        let item = ItemBuilder::default()
            .title(title)
            .author(message.sender)
            .link(message.link)
            .source(Source {
                url: homeserver_url.clone(),
                title: Some(homeserver_url.clone()),
            })
            .pub_date(message.time.to_rfc2822())
            //.enclosure(enclosure)
            .description(message.content.clone())
            .content(message.content)
            .build();

        items.push(item);
    }

    channel.set_items(items);

    let now = Utc::now().to_rfc2822();
    channel.set_pub_date(now.clone());
    channel.set_last_build_date(now);

    channel.to_string()
}