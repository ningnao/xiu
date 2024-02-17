use std::collections::HashMap;
use streamhub::define::StreamHubEventSender;

use super::session::server_session;
use commonlib::auth::Auth;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::Error;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct RtmpServer {
    address: String,
    event_producer: StreamHubEventSender,
    gop_num: usize,
    auth: Option<Auth>,
    enabled_nonce: bool,
    publish_token: Option<String>,
    subscribe_token: Option<String>,
    nonce_map: Arc<Mutex<HashMap<String, i64>>>,
}

impl RtmpServer {
    pub fn new(
        address: String,
        event_producer: StreamHubEventSender,
        gop_num: usize,
        auth: Option<Auth>,
        publish_token: Option<String>,
        subscribe_token: Option<String>,
        enabled_nonce: bool,
        nonce_map: Arc<Mutex<HashMap<String, i64>>>
    ) -> Self {
        Self {
            address,
            event_producer,
            gop_num,
            auth,
            publish_token,
            subscribe_token,
            enabled_nonce,
            nonce_map,
        }
    }

    pub async fn run(&mut self) -> Result<(), Error> {
        let socket_addr: &SocketAddr = &self.address.parse().unwrap();
        let listener = TcpListener::bind(socket_addr).await?;

        log::info!("Rtmp server listening on tcp://{}", socket_addr);
        loop {
            let (tcp_stream, _) = listener.accept().await?;
            //tcp_stream.set_keepalive(Some(Duration::from_secs(30)))?;

            let mut session = server_session::ServerSession::new(
                tcp_stream,
                self.event_producer.clone(),
                self.gop_num,
                self.auth.clone(),
                self.publish_token.clone(),
                self.subscribe_token.clone(),
                self.enabled_nonce,
                Arc::clone(&self.nonce_map),
            );
            tokio::spawn(async move {
                if let Err(err) = session.run().await {
                    log::info!(
                        "session run error: session_type: {}, app_name: {}, stream_name: {}, err: {}",
                        session.common.session_type,
                        session.app_name,
                        session.stream_name,
                        err
                    );
                }
            });
        }
    }
}
