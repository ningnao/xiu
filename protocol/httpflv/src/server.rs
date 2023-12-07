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
    subscribe_token: Option<String>,
) -> Result<Response<Body>> {
    let path = req.uri().path();

    match path.find(".flv") {
        Some(index) if index > 0 => {
            let (left, _) = path.split_at(index);
            let rv: Vec<_> = left.split('/').collect();

            let app_name = String::from(rv[1]);
            let stream_name = String::from(rv[2]);

            let (http_response_data_producer, http_response_data_consumer) = unbounded();

            let mut flv_hanlder = HttpFlv::new(
                app_name,
                stream_name,
                event_producer,
                http_response_data_producer,
                req,
                remote_addr,
                need_record,
                subscribe_token,
            );

            tokio::spawn(async move {
                if let Err(err) = flv_hanlder.run().await {
                    log::error!("flv handler run error: {}", err);
                }
                let _ = flv_hanlder.unsubscribe_from_rtmp_channels().await;
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

pub async fn run(event_producer: StreamHubEventSender, port: usize, need_record: bool, subscribe_token: Option<String>) -> Result<()> {
    let listen_address = format!("0.0.0.0:{port}");
    let sock_addr = listen_address.parse().unwrap();

    let new_service = make_service_fn(move |socket: &AddrStream| {
        let remote_addr = socket.remote_addr();
        let flv_copy = event_producer.clone();
        let subscribe_token = subscribe_token.clone();
        async move {
            Ok::<_, GenericError>(service_fn(move |req| {
                handle_connection(req, flv_copy.clone(), remote_addr, need_record, subscribe_token.clone())
            }))
        }
    });

    let server = Server::bind(&sock_addr).serve(new_service);

    log::info!("Httpflv server listening on http://{}", sock_addr);

    server.await?;

    Ok(())
}
