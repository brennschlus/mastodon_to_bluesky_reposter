#![feature(async_closure)]

use html_entities::decode_html_entities;
use megalodon::{self, streaming::Message};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Deserialize;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mastodon_token = std::env::var("MASTODON_TOKEN")?;

    let mastodon_client = megalodon::generator(
        megalodon::SNS::Mastodon,
        String::from("https://mas.to"),
        Some(mastodon_token),
        None,
    );

    let stream =
        mastodon_client.user_streaming(String::from("wss://mas.to/api/v1/streaming/brennschluss"));

    stream
        .listen(Box::new(|message| {
            tokio::spawn(async move {
                if let Err(e) = polling_messages(message).await {
                    eprintln!("Error: {:?}", e);
                }
            });
        }))
        .await;
    Ok(())
}

async fn polling_messages(message: Message) -> Result<(), Box<dyn std::error::Error>> {
    match message {
        Message::Update(mes) => {
            let decoded_message = decode_html_entities(&mes.content).unwrap();
            let re = Regex::new(r"<[^>]+>").unwrap();

            let plain_text = re.replace_all(&decoded_message, "");

            if plain_text.is_empty() {
                return Ok(());
            }

            sent_post_to_bluesky(&plain_text).await?;

            Ok(())
        }
        _ => Ok(()),
    }
}

async fn sent_post_to_bluesky(post: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bluesky_handle = std::env::var("BLUESKY_HANDLE")?;
    let bluesky_password = std::env::var("BLUESKY_PASSWORD")?;
    let client = reqwest::Client::new();
    let resp = client
        .post("https://bsky.social/xrpc/com.atproto.server.createSession")
        .json(&json!({
            "identifier": bluesky_handle, "password": bluesky_password
        }))
        .send()
        .await?;

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    struct Session {
        access_jwt: String,
        did: String,
    }
    let session = serde_json::from_str::<Session>(&resp.text().await?)?;

    let now = chrono::offset::Utc::now().to_rfc3339();
    let post = json!(
        {
            "$type": "app.bsky.feed.post",
            "text": post,
            "createdAt": now,
        }
    );

    let acces_jwt = format!("Bearer {}", session.access_jwt);
    let mut headers = HeaderMap::new();

    headers.insert(AUTHORIZATION, HeaderValue::from_str(&acces_jwt)?);

    client
        .post("https://bsky.social/xrpc/com.atproto.repo.createRecord")
        .headers(headers)
        .json(&json!({
            "repo": session.did,
            "collection": "app.bsky.feed.post",
            "record": post,
        }))
        .send()
        .await?;

    Ok(())
}
