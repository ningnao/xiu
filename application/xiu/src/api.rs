use std::collections::HashMap;
use chrono::Local;
use tokio::sync::Mutex;
use {
    anyhow::Result,
    axum::{
        routing::{get, post},
        Json, Router,
    },
    serde::Deserialize,
    std::sync::Arc,
    streamhub::{define, define::StreamHubEventSender, utils::Uuid},
    {
        tokio,
        tokio::sync::{mpsc, oneshot},
    },
};

// the input to our `KickOffClient` handler
#[derive(Deserialize)]
struct KickOffClient {
    id: String,
}

#[derive(Clone)]
struct ApiService {
    channel_event_producer: StreamHubEventSender,
}

impl ApiService {
    async fn root(&self) -> String {
        String::from(
            "Usage of xiu http api:
                ./get_stream_status(get)  get audio and video stream statistic information.
                ./kick_off_client(post) kick off client by publish/subscribe id.\n",
        )
    }

    async fn get_stream_status(&self) -> Result<String> {
        let (data_sender, mut data_receiver) = mpsc::unbounded_channel();
        let (size_sender, size_receiver) = oneshot::channel();
        let hub_event = define::StreamHubEvent::ApiStatistic {
            data_sender,
            size_sender,
        };
        if let Err(err) = self.channel_event_producer.send(hub_event) {
            log::error!("send api event error: {}", err);
        }
        let mut data = Vec::new();
        match size_receiver.await {
            Ok(size) => {
                if size == 0 {
                    return Ok(String::from("no stream data"));
                }
                loop {
                    if let Some(stream_statistics) = data_receiver.recv().await {
                        data.push(stream_statistics);
                    }
                    if data.len() == size {
                        break;
                    }
                }
            }
            Err(err) => {
                log::error!("start_api_service recv size error: {}", err);
            }
        }

        if let Ok(data) = serde_json::to_string(&data) {
            return Ok(data);
        }

        Ok(String::from(""))
    }

    async fn kick_off_client(&self, id: KickOffClient) -> Result<String> {
        let id_result = Uuid::from_str2(&id.id);

        if let Some(id) = id_result {
            let hub_event = define::StreamHubEvent::ApiKickClient { id };

            if let Err(err) = self.channel_event_producer.send(hub_event) {
                log::error!("send api kick_off_client event error: {}", err);
            }
        }

        Ok(String::from("ok"))
    }

    async fn gen_nonce(&self, nonce_map: &Arc<Mutex<HashMap<String, i64>>>) -> String {
        let nonce = uuid::Uuid::new_v4().to_string();
        nonce_map.lock().await.insert(nonce.clone(), Local::now().timestamp_millis() + (10 * 60 * 1000));
        nonce
    }
}

pub async fn run(producer: StreamHubEventSender, port: usize, nonce_map: Arc<Mutex<HashMap<String, i64>>>) {
    let api = Arc::new(ApiService {
        channel_event_producer: producer,
    });

    let api_root = api.clone();
    let root = move || async move { api_root.root().await };

    let get_status = api.clone();
    let status = move || async move {
        get_status.get_stream_status().await.unwrap_or_else(|_| "error".to_owned())
    };

    let kick_off = api.clone();
    let kick = move |Json(id): Json<KickOffClient>| async move {
        kick_off.kick_off_client(id).await.unwrap_or_else(|_| "error".to_owned())
    };

    let gen_nonce_api = api.clone();
    let nonce_map_clone = Arc::clone(&nonce_map);
    let gen_nonce = move || async move { gen_nonce_api.gen_nonce(&nonce_map_clone).await };

    let app = Router::new()
        .route("/", get(root))
        .route("/get_stream_status", get(status))
        .route("/kick_off_client", post(kick))
        .route("/gen_nonce", post(gen_nonce));

    log::info!("Http api server listening on http://0.0.0.0:{}", port);
    axum::Server::bind(&([0, 0, 0, 0], port as u16).into())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
