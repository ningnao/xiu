use std::collections::HashMap;
use chrono::Local;
use {
    super::httpflv::HttpFlv,
    futures::channel::mpsc::unbounded,
    hyper::{
        server::conn::AddrStream,
        service::{make_service_fn, service_fn},
        Body, Request, Response, Server, StatusCode,
    },
    std::net::SocketAddr,
    streamhub::define::StreamHubEventSender,
};

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type Result<T> = std::result::Result<T, GenericError>;
static NOTFOUND: &[u8] = b"Not Found";

async fn handle_connection(
    req: Request<Body>,
    event_producer: StreamHubEventSender, // event_producer: ChannelEventProducer
    remote_addr: SocketAddr,
    need_record: bool,
) -> Result<Response<Body>> {
    let path = req.uri().path();

    match path.find(".flv") {
        Some(index) if index > 0 => {
            let (left, _) = path.split_at(index);
            let rv: Vec<_> = left.split('/').collect();

            let app_name = String::from(rv[1]);
            let stream_name = String::from(rv[2]);

            let (http_response_data_producer, http_response_data_consumer) = unbounded();

            let flv_name = {
                if let Some(params) = req.uri().query() {
                    let mut params_map: HashMap<_, _> = HashMap::new();

                    for param in params.split("&") {
                        let entry: Vec<_> = param.split("=").collect();

                        if entry.len() == 2 {
                            params_map.insert(entry[0].to_string(), entry[1].to_string());
                        }
                    }
                    params_map.remove("file_name")
                } else {
                    None
                }
            };

            let flv_name = match flv_name {
                Some(flv_name) => format!("{}.flv",flv_name),
                None => format!("{}-{}.flv", stream_name, Local::now().format("%Y-%m-%d-%H-%M-%S").to_string())
            };

            let mut flv_hanlder = HttpFlv::new(
                app_name,
                stream_name,
                event_producer,
                http_response_data_producer,
                req.uri().to_string(),
                remote_addr,
                flv_name,
                need_record,
            );

            tokio::spawn(async move {
                if let Err(err) = flv_hanlder.run().await {
                    log::error!("flv handler run error {}", err);
                }
            });

            let mut resp = Response::new(Body::wrap_stream(http_response_data_consumer));
            resp.headers_mut()
                .insert("Access-Control-Allow-Origin", "*".parse().unwrap());

            Ok(resp)
        }

        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(NOTFOUND.into())
            .unwrap()),
    }
}

pub async fn run(event_producer: StreamHubEventSender, port: usize, need_record: bool) -> Result<()> {
    let listen_address = format!("0.0.0.0:{port}");
    let sock_addr = listen_address.parse().unwrap();

    let new_service = make_service_fn(move |socket: &AddrStream| {
        let remote_addr = socket.remote_addr();
        let flv_copy = event_producer.clone();
        async move {
            Ok::<_, GenericError>(service_fn(move |req| {
                handle_connection(req, flv_copy.clone(), remote_addr, need_record)
            }))
        }
    });

    let server = Server::bind(&sock_addr).serve(new_service);

    log::info!("Httpflv server listening on http://{}", sock_addr);

    server.await?;

    Ok(())
}
