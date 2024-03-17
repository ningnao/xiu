use std::collections::HashMap;
use chrono::Local;
use tokio::sync::Mutex;
use {
    anyhow::Result,
    axum::{
        extract::Query,
        routing::{get, post},
        Json, Router,
    },
    serde::Deserialize,
    serde_json::Value,
    std::sync::Arc,
    streamhub::{
        define::{self, StreamHubEventSender},
        stream::StreamIdentifier,
        utils::Uuid,
    },
    tokio::{self, sync::oneshot},
};

#[derive(serde::Serialize)]
struct ApiResponse<T> {
    error_code: i32,
    desp: String,
    data: T,
}

// the input to our `KickOffClient` handler
#[derive(Deserialize)]
struct KickOffClient {
    uuid: String,
}

#[derive(Deserialize, Debug)]
struct QueryWholeStreamsParams {
    // query top N by subscriber's count.
    top: Option<usize>,
}

#[derive(Deserialize)]
struct QueryStream {
    identifier: StreamIdentifier,
    // if specify uuid, then query the stream by uuid and filter no used data.
    uuid: Option<String>,
}

#[derive(Clone)]
struct ApiService {
    channel_event_producer: StreamHubEventSender,
}

impl ApiService {
    async fn root(&self) -> String {
        String::from(
            "Usage of xiu http api:
                ./query_whole_streams(get) query whole streams' information or top streams' information.
                ./query_stream(post) query stream information by identifier and uuid.
                ./kick_off_client(post) kick off client by publish/subscribe id.
                ./gen_nonce(post) generation nonce for publish/subscribe client.\n",
        )
    }

    async fn query_whole_streams(
        &self,
        params: QueryWholeStreamsParams,
    ) -> Json<ApiResponse<Value>> {
        log::info!("query_whole_streams: {:?}", params);
        let (result_sender, result_receiver) = oneshot::channel();
        let hub_event = define::StreamHubEvent::ApiStatistic {
            top_n: params.top,
            identifier: None,
            uuid: None,
            result_sender,
        };
        if let Err(err) = self.channel_event_producer.send(hub_event) {
            log::error!("send api event error: {}", err);
        }

        match result_receiver.await {
            Ok(dat_val) => {
                let api_response = ApiResponse {
                    error_code: 0,
                    desp: String::from("succ"),
                    data: dat_val,
                };
                Json(api_response)
            }
            Err(err) => {
                let api_response = ApiResponse {
                    error_code: -1,
                    desp: String::from("failed"),
                    data: serde_json::json!(err.to_string()),
                };
                Json(api_response)
            }
        }
    }

    async fn query_stream(&self, stream: QueryStream) -> Json<ApiResponse<Value>> {
        let uuid = if let Some(uid) = stream.uuid {
            Uuid::from_str2(&uid)
        } else {
            None
        };

        let (result_sender, result_receiver) = oneshot::channel();
        let hub_event = define::StreamHubEvent::ApiStatistic {
            top_n: None,
            identifier: Some(stream.identifier),
            uuid,
            result_sender,
        };

        if let Err(err) = self.channel_event_producer.send(hub_event) {
            log::error!("send api event error: {}", err);
        }

        match result_receiver.await {
            Ok(dat_val) => {
                let api_response = ApiResponse {
                    error_code: 0,
                    desp: String::from("succ"),
                    data: dat_val,
                };
                Json(api_response)
            }
            Err(err) => {
                let api_response = ApiResponse {
                    error_code: -1,
                    desp: String::from("failed"),
                    data: serde_json::json!(err.to_string()),
                };
                Json(api_response)
            }
        }
    }

    async fn kick_off_client(&self, id: KickOffClient) -> Result<String> {
        let id_result = Uuid::from_str2(&id.uuid);

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

    let api_query_streams = api.clone();
    let query_streams = move |Query(params): Query<QueryWholeStreamsParams>| async move {
        api_query_streams.query_whole_streams(params).await
    };

    let api_query_stream = api.clone();
    let query_stream = move |Json(stream): Json<QueryStream>| async move {
        api_query_stream.query_stream(stream).await
    };

    let api_kick_off = api.clone();
    let kick_off = move |Json(id): Json<KickOffClient>| async move {
        api_kick_off.kick_off_client(id).await.unwrap_or_else(|_| "error".to_owned())
    };

    let gen_nonce_api = api.clone();
    let nonce_map_clone = Arc::clone(&nonce_map);
    let gen_nonce = move || async move { gen_nonce_api.gen_nonce(&nonce_map_clone).await };

    let app = Router::new()
        .route("/", get(root))
        .route("/query_whole_streams", get(query_streams))
        .route("/query_stream", post(query_stream))
        .route("/kick_off_client", post(kick_off))
        .route("/gen_nonce", post(gen_nonce));

    log::info!("Http api server listening on http://0.0.0.0:{}", port);
    axum::Server::bind(&([0, 0, 0, 0], port as u16).into())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
