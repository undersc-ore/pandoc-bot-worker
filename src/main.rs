use std::{env, sync::Arc};

use anyhow::{Context, Result};
use futures_lite::StreamExt;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use tokio::{fs, process::Command};

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();

    let amqp_url = env::var("AMQP_URL").context("No AMQP_URL provided")?;
    let amqp_conn = lapin::Connection::connect(
        &amqp_url,
        lapin::ConnectionProperties::default()
            .with_executor(tokio_executor_trait::Tokio::current())
            .with_reactor(tokio_reactor_trait::Tokio),
    )
    .await?;
    let amqp_conn = Arc::new(amqp_conn);

    let channel = amqp_conn.create_channel().await?;
    let queue = channel
        .queue_declare("pandoc-bot-jobs", Default::default(), Default::default())
        .await?;
    info!("Declared queue {queue:?}");

    let mut consumer = channel
        .basic_consume(
            "pandoc-bot-jobs",
            "",
            Default::default(),
            Default::default(),
        )
        .await?;
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery?;

        let req: ConvertRequest = bson::from_slice(&delivery.data)?;
        delivery.ack(Default::default()).await?;
        info!("Consumed request");

        fs::write(&req.file_id, &req.file).await?;

        let out_filename = format!("{}.out", req.file_id);
        let cmdargs = vec![
            req.file_id.clone(),
            "-o".to_string(),
            out_filename.clone(),
            "--from".to_string(),
            req.from_filetype.clone(),
            "--to".to_string(),
            req.to_filetype.clone(),
        ];

        info!("Running pandoc for {}", req.file_id);
        let output = Command::new("pandoc").args(cmdargs).output();
        let output = output.await?;

        let pub_channel = amqp_conn.create_channel().await?;
        if output.status.success() {
            info!("Pandoc subprocess succeeded for {}", req.file_id);

            let out_file_bytes = fs::read(&out_filename).await?;

            // Create response and convert to BSON
            let res = {
                let res = ConvertResponse::Success {
                    chat_id: req.chat_id,
                    file: out_file_bytes,
                    to_filetype: req.to_filetype,
                };
                bson::to_vec(&res)?
            };

            // Send to queue
            pub_channel
                .basic_publish(
                    "",
                    "pandoc-outputs",
                    Default::default(),
                    &res,
                    Default::default(),
                )
                .await?
                .await?;

            info!("Replied to MQ for {}", req.file_id);
        } else {
            let error_msg = String::from_utf8_lossy(&output.stdout).to_string()
                + &String::from_utf8_lossy(&output.stderr).to_string();

            warn!("Pandoc subprocess failed for {}", req.file_id);
            warn!("{error_msg}");

            // Create response and convert to BSON
            let res = {
                let res = ConvertResponse::Failure {
                    chat_id: req.chat_id,
                    error_msg,
                };
                bson::to_vec(&res)?
            };

            // Send to queue
            pub_channel
                .basic_publish(
                    "",
                    "pandoc-outputs",
                    Default::default(),
                    &res,
                    Default::default(),
                )
                .await?
                .await?;
        }
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct ConvertRequest {
    chat_id: i64,
    #[serde(with = "serde_bytes")]
    file: Vec<u8>,
    file_id: String,
    from_filetype: String,
    to_filetype: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum ConvertResponse {
    Success {
        chat_id: i64,
        #[serde(with = "serde_bytes")]
        file: Vec<u8>,
        to_filetype: String,
    },
    Failure {
        chat_id: i64,
        error_msg: String,
    },
}
